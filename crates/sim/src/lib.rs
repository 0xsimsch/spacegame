//! Authoritative game simulation: physics world, grids, blocks, players and scripts.
//! Has no knowledge of networking or rendering.

mod build;
mod control;
pub mod prefabs;
mod save;
mod scripting;

use rapier3d_f64::prelude::*;
use shared::blocks::BlockKind;
use shared::protocol::{BlockControlInfo, BlockData, PlayerInput};
use shared::{BlockId, DQuat, DVec3, GridId, IVec3, PlayerId, TICK_DT};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::time::Instant;

pub use save::SaveData;

/// Bit set in rigid-body `user_data` for players (otherwise it is a grid id).
const PLAYER_FLAG: u128 = 1 << 64;
const PLANET_FLAG: u128 = 1 << 65;

pub const PLAYER_RADIUS: f64 = 0.45;
pub const PLAYER_MASS: f64 = 100.0;
pub const JETPACK_ACCEL: f64 = 12.0;
pub const WALK_SPEED: f64 = 6.0;
pub const INTERACT_RANGE: f64 = 12.0;
/// Half-size of a block collider (slightly under 0.5 so neighbouring grids don't touch).
pub const BLOCK_HALF: f64 = 0.48;

#[derive(Debug)]
pub struct Thruster {
    /// Manual/script override (0..1). Takes precedence over cockpit demand when > 0.
    pub override_: f64,
    /// Demand from a piloted cockpit (0..1), recomputed every tick.
    pub demand: f64,
    /// Actual output (0..1) applied last tick.
    pub output: f64,
}

#[derive(Debug)]
pub struct Mech {
    pub top: Option<GridId>,
    pub joint: Option<ImpulseJointHandle>,
    /// Piston: m/s. Hinge: deg/s.
    pub velocity: f64,
    /// Hinge only: target angle in degrees (overrides `velocity`).
    pub target: Option<f64>,
    /// Piston extension (m) or hinge angle (deg), read back after each step.
    pub value: f64,
}

pub struct Program {
    pub source: String,
    pub host: Option<script::ScriptHost>,
    pub output: VecDeque<String>,
    pub error: Option<String>,
    pub frequency: u32,
    pub storage: String,
    pub pending_messages: Vec<String>,
    /// Output produced since the last flush to clients.
    pub new_output: Vec<String>,
    pub output_dirty: bool,
    pub last_run_us: u64,
}

impl std::fmt::Debug for Program {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Program").field("error", &self.error).finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub enum BlockState {
    Plain,
    Thruster(Thruster),
    Mech(Mech),
    Program(Box<Program>),
    Cockpit { pilot: Option<PlayerId> },
}

#[derive(Debug)]
pub struct Block {
    pub id: BlockId,
    pub kind: BlockKind,
    pub cell: IVec3,
    pub rot: u8,
    pub name: String,
    pub collider: ColliderHandle,
    pub state: BlockState,
}

impl Block {
    pub fn data(&self) -> BlockData {
        BlockData { id: self.id, kind: self.kind, cell: self.cell, rot: self.rot, name: self.name.clone() }
    }
}

#[derive(Debug)]
pub struct Grid {
    pub id: GridId,
    pub body: RigidBodyHandle,
    pub is_static: bool,
    pub blocks: HashMap<IVec3, Block>,
    pub cruise: bool,
    /// If this grid is the head of a piston/hinge: the mechanical block it hangs on.
    pub mount: Option<BlockId>,
}

#[derive(Debug)]
pub struct Player {
    pub id: PlayerId,
    pub name: String,
    pub body: RigidBodyHandle,
    pub collider: ColliderHandle,
    pub input: PlayerInput,
    pub seat: Option<BlockId>,
    pub grounded: bool,
}

/// Things that happened during a step that clients may need to hear about.
#[derive(Clone, Debug)]
pub enum WorldEvent {
    GridAdded(GridId),
    GridRemoved(GridId),
    BlockAdded { grid: GridId, block: BlockData },
    BlockRemoved { grid: GridId, block: BlockId },
    BlockRenamed { grid: GridId, block: BlockId, name: String },
    BlockControl { grid: GridId, info: BlockControlInfo },
    ScriptOutput { grid: GridId, block: BlockId, lines: Vec<String>, error: Option<String> },
    /// Message for one player (errors from commands etc.).
    Notice { player: PlayerId, text: String },
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StepStats {
    pub tick_ms: f32,
    pub script_ms: f32,
    pub physics_ms: f32,
    pub scripts_run: u32,
}

pub struct World {
    pub phys: PhysicsWorld,
    pub tick: u64,
    pub grids: BTreeMap<GridId, Grid>,
    pub players: BTreeMap<PlayerId, Player>,
    /// Where every block lives.
    pub block_loc: HashMap<BlockId, (GridId, IVec3)>,
    pub next_grid: GridId,
    pub next_block: BlockId,
    pub events: Vec<WorldEvent>,
    pub stats: StepStats,
    /// Player positions by name, restored when they reconnect.
    pub saved_players: HashMap<String, DVec3>,
    dirty_controls: HashSet<BlockId>,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

pub fn pose(pos: DVec3, rot: DQuat) -> Pose {
    Pose::from_parts(pos, rot)
}

impl World {
    pub fn new() -> Self {
        let mut phys = PhysicsWorld::new();
        phys.gravity = DVec3::ZERO;
        phys.integration_parameters.dt = TICK_DT;
        for (i, b) in shared::solar::BODIES.iter().enumerate() {
            let body = RigidBodyBuilder::fixed()
                .translation(b.position)
                .user_data(PLANET_FLAG | i as u128)
                .build();
            let col = ColliderBuilder::ball(b.radius).friction(0.9).build();
            phys.insert(body, col);
        }
        Self {
            phys,
            tick: 0,
            grids: BTreeMap::new(),
            players: BTreeMap::new(),
            block_loc: HashMap::new(),
            next_grid: 1,
            next_block: 1,
            events: Vec::new(),
            stats: StepStats::default(),
            saved_players: HashMap::new(),
            dirty_controls: HashSet::new(),
        }
    }

    // ---------------------------------------------------------------- players

    pub fn add_player(&mut self, id: PlayerId, name: &str) {
        let pos = self
            .saved_players
            .get(name)
            .copied()
            .unwrap_or_else(|| shared::solar::default_spawn() + DVec3::X * (id % 8) as f64 * 2.0);
        let body = RigidBodyBuilder::dynamic()
            .translation(pos)
            .lock_rotations()
            .can_sleep(false)
            .ccd_enabled(true)
            .user_data(PLAYER_FLAG | id as u128)
            .build();
        let col = ColliderBuilder::ball(PLAYER_RADIUS).mass(PLAYER_MASS).friction(0.8).build();
        let (body, collider) = self.phys.insert(body, col);
        self.players.insert(
            id,
            Player {
                id,
                name: name.to_string(),
                body,
                collider,
                input: PlayerInput { look: DQuat::IDENTITY, dampeners: true, jetpack: true, ..Default::default() },
                seat: None,
                grounded: false,
            },
        );
    }

    pub fn remove_player(&mut self, id: PlayerId) {
        self.exit_seat(id);
        if let Some(p) = self.players.remove(&id) {
            let pos = self.phys.bodies[p.body].translation();
            self.saved_players.insert(p.name.clone(), pos);
            self.phys.remove_body(p.body);
        }
    }

    pub fn set_input(&mut self, id: PlayerId, input: PlayerInput) {
        if let Some(p) = self.players.get_mut(&id) {
            // Drop out-of-order packets.
            if input.seq >= p.input.seq || p.input.seq - input.seq > 1_000_000 {
                p.input = input;
            }
        }
    }

    pub fn player_pos(&self, id: PlayerId) -> Option<DVec3> {
        self.players.get(&id).map(|p| self.phys.bodies[p.body].translation())
    }

    pub fn teleport_player(&mut self, id: PlayerId, pos: DVec3) {
        self.exit_seat(id);
        if let Some(p) = self.players.get(&id) {
            let b = &mut self.phys.bodies[p.body];
            b.set_translation(pos, true);
            b.set_linvel(DVec3::ZERO, true);
        }
    }

    // ---------------------------------------------------------------- queries

    pub fn grid_pose(&self, id: GridId) -> Option<Pose> {
        self.grids.get(&id).map(|g| *self.phys.bodies[g.body].position())
    }

    pub fn block(&self, id: BlockId) -> Option<(&Grid, &Block)> {
        let (gid, cell) = self.block_loc.get(&id)?;
        let g = self.grids.get(gid)?;
        Some((g, g.blocks.get(cell)?))
    }

    pub fn block_mut(&mut self, id: BlockId) -> Option<&mut Block> {
        let (gid, cell) = self.block_loc.get(&id)?;
        self.grids.get_mut(gid)?.blocks.get_mut(cell)
    }

    /// World-space centre of a block.
    pub fn block_world_pos(&self, id: BlockId) -> Option<DVec3> {
        let (g, b) = self.block(id)?;
        Some(self.phys.bodies[g.body].position().transform_point(b.cell.as_dvec3()))
    }

    /// The set of grids mechanically connected to `grid` (through pistons/hinges).
    pub fn mech_group(&self, grid: GridId) -> Vec<GridId> {
        let mut seen = vec![grid];
        let mut i = 0;
        while i < seen.len() {
            let g = &self.grids[&seen[i]];
            let mut next = Vec::new();
            if let Some(m) = g.mount {
                if let Some((pg, _)) = self.block_loc.get(&m) {
                    next.push(*pg);
                }
            }
            for b in g.blocks.values() {
                if let BlockState::Mech(Mech { top: Some(t), .. }) = b.state {
                    next.push(t);
                }
            }
            for n in next {
                if !seen.contains(&n) && self.grids.contains_key(&n) {
                    seen.push(n);
                }
            }
            i += 1;
        }
        seen
    }

    pub fn control_info(&self, block: BlockId) -> Option<BlockControlInfo> {
        let (_, b) = self.block(block)?;
        let mut info = BlockControlInfo {
            block,
            thruster_override: 0.0,
            piston_velocity: 0.0,
            hinge_velocity: 0.0,
            hinge_target: None,
        };
        match &b.state {
            BlockState::Thruster(t) => info.thruster_override = t.override_ as f32,
            BlockState::Mech(m) if b.kind == BlockKind::Piston => info.piston_velocity = m.velocity as f32,
            BlockState::Mech(m) => {
                info.hinge_velocity = m.velocity as f32;
                info.hinge_target = m.target.map(|t| t as f32);
            }
            _ => {}
        }
        Some(info)
    }

    pub(crate) fn mark_control_dirty(&mut self, block: BlockId) {
        self.dirty_controls.insert(block);
    }

    pub fn total_blocks(&self) -> usize {
        self.block_loc.len()
    }

    pub fn program_count(&self) -> usize {
        self.grids
            .values()
            .flat_map(|g| g.blocks.values())
            .filter(|b| matches!(b.state, BlockState::Program(_)))
            .count()
    }

    // ---------------------------------------------------------------- step

    pub fn step(&mut self) {
        let t0 = Instant::now();
        let ts = Instant::now();
        let scripts_run = self.run_scripts();
        let script_ms = ts.elapsed().as_secs_f32() * 1000.0;

        self.apply_player_forces();
        self.apply_cockpit_control();
        self.apply_grid_forces();
        self.update_mech_motors();

        let tp = Instant::now();
        self.phys.step();
        let physics_ms = tp.elapsed().as_secs_f32() * 1000.0;

        self.post_step();
        self.tick += 1;
        if self.tick % 10 == 0 {
            self.flush_throttled_events();
        }
        self.stats = StepStats {
            tick_ms: t0.elapsed().as_secs_f32() * 1000.0,
            script_ms,
            physics_ms,
            scripts_run,
        };
    }

    fn flush_throttled_events(&mut self) {
        let dirty: Vec<BlockId> = self.dirty_controls.drain().collect();
        for id in dirty {
            if let (Some(info), Some((gid, _))) = (self.control_info(id), self.block_loc.get(&id)) {
                self.events.push(WorldEvent::BlockControl { grid: *gid, info });
            }
        }
        let mut out = Vec::new();
        for g in self.grids.values_mut() {
            for b in g.blocks.values_mut() {
                if let BlockState::Program(p) = &mut b.state {
                    if p.output_dirty || !p.new_output.is_empty() {
                        out.push(WorldEvent::ScriptOutput {
                            grid: g.id,
                            block: b.id,
                            lines: std::mem::take(&mut p.new_output),
                            error: p.error.clone(),
                        });
                        p.output_dirty = false;
                    }
                }
            }
        }
        self.events.extend(out);
    }

    fn post_step(&mut self) {
        // Speed caps.
        for g in self.grids.values() {
            let cap = if g.cruise { shared::CRUISE_SPEED_CAP } else { shared::SPEED_CAP };
            let b = &mut self.phys.bodies[g.body];
            let v = b.linvel();
            if v.length() > cap {
                b.set_linvel(v.normalize() * cap, false);
            }
        }
        for p in self.players.values() {
            let b = &mut self.phys.bodies[p.body];
            let v = b.linvel();
            if v.length() > shared::SPEED_CAP {
                b.set_linvel(v.normalize() * shared::SPEED_CAP, false);
            }
        }
        self.read_mech_values();
        self.update_seated_players();
        // Grounded detection.
        let ids: Vec<PlayerId> = self.players.keys().copied().collect();
        for id in ids {
            let p = &self.players[&id];
            let grounded = p.seat.is_none()
                && self.phys.contact_pairs_with(p.collider).any(|c| c.has_any_active_contact());
            self.players.get_mut(&id).unwrap().grounded = grounded;
        }
    }

    pub fn drain_events(&mut self) -> Vec<WorldEvent> {
        std::mem::take(&mut self.events)
    }
}

#[cfg(test)]
mod tests;
