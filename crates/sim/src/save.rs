//! Saving and loading the world (postcard-encoded).

use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub enum SavedState {
    Plain,
    Thruster { override_: f64 },
    Mech { top: Option<GridId>, velocity: f64, target: Option<f64>, value: f64, free: bool },
    Program { source: String, storage: String },
    Cockpit,
    Gyro { override_: Option<DVec3> },
    Door { open: bool },
    Sensor { range: f64 },
    Gear { auto_lock: bool, lock: Option<SavedLock> },
    Connector { auto_lock: bool, lock: Option<SavedLock>, partner: Option<BlockId> },
    Merge { enabled: bool },
}

/// A lock joint: what it holds and the joint frames in both bodies.
#[derive(Serialize, Deserialize)]
pub struct SavedLock {
    pub target: LockTarget,
    pub pos1: DVec3,
    pub rot1: DQuat,
    pub pos2: DVec3,
    pub rot2: DQuat,
}

fn save_lock(l: &Option<Lock>) -> Option<SavedLock> {
    l.as_ref().map(|l| SavedLock {
        target: l.target,
        pos1: l.frame1.translation,
        rot1: l.frame1.rotation,
        pos2: l.frame2.translation,
        rot2: l.frame2.rotation,
    })
}

#[derive(Serialize, Deserialize)]
pub struct SavedBlock {
    pub id: BlockId,
    pub kind: BlockKind,
    pub cell: IVec3,
    pub rot: u8,
    pub name: String,
    pub state: SavedState,
}

#[derive(Serialize, Deserialize)]
pub struct SavedGrid {
    pub id: GridId,
    pub is_static: bool,
    pub pos: DVec3,
    pub rot: DQuat,
    pub linvel: DVec3,
    pub angvel: DVec3,
    pub cruise: bool,
    pub mount: Option<BlockId>,
    pub blocks: Vec<SavedBlock>,
}

#[derive(Serialize, Deserialize)]
pub struct SaveData {
    pub version: u32,
    pub tick: u64,
    pub next_grid: GridId,
    pub next_block: BlockId,
    pub grids: Vec<SavedGrid>,
    pub players: Vec<(String, DVec3)>,
}

const SAVE_VERSION: u32 = 4;

impl World {
    pub fn save(&self) -> SaveData {
        let mut players: HashMap<String, DVec3> = self.saved_players.clone();
        for p in self.players.values() {
            players.insert(p.name.clone(), self.phys.bodies[p.body].translation());
        }
        let grids = self
            .grids
            .values()
            .map(|g| {
                let body = &self.phys.bodies[g.body];
                let mut blocks: Vec<SavedBlock> = g
                    .blocks
                    .values()
                    .map(|b| SavedBlock {
                        id: b.id,
                        kind: b.kind,
                        cell: b.cell,
                        rot: b.rot,
                        name: b.name.clone(),
                        state: match &b.state {
                            BlockState::Plain => SavedState::Plain,
                            BlockState::Thruster(t) => SavedState::Thruster { override_: t.override_ },
                            BlockState::Mech(m) => SavedState::Mech { top: m.top, velocity: m.velocity, target: m.target, value: m.value, free: m.free },
                            BlockState::Program(p) => SavedState::Program { source: p.source.clone(), storage: p.storage.clone() },
                            BlockState::Cockpit { .. } => SavedState::Cockpit,
                            BlockState::Gyro { override_ } => SavedState::Gyro { override_: *override_ },
                            BlockState::Door { open } => SavedState::Door { open: *open },
                            BlockState::Sensor { range, .. } => SavedState::Sensor { range: *range },
                            BlockState::Gear(a) => SavedState::Gear { auto_lock: a.auto_lock, lock: save_lock(&a.lock) },
                            BlockState::Connector(a) => {
                                SavedState::Connector { auto_lock: a.auto_lock, lock: save_lock(&a.lock), partner: a.partner }
                            }
                            BlockState::Merge { enabled, .. } => SavedState::Merge { enabled: *enabled },
                        },
                    })
                    .collect();
                blocks.sort_by_key(|b| b.id);
                SavedGrid {
                    id: g.id,
                    is_static: g.is_static,
                    pos: body.translation(),
                    rot: *body.rotation(),
                    linvel: body.linvel(),
                    angvel: body.angvel(),
                    cruise: g.cruise,
                    mount: g.mount,
                    blocks,
                }
            })
            .collect();
        SaveData {
            version: SAVE_VERSION,
            tick: self.tick,
            next_grid: self.next_grid,
            next_block: self.next_block,
            grids,
            players: players.into_iter().collect(),
        }
    }

    pub fn save_bytes(&self) -> Vec<u8> {
        postcard::to_allocvec(&self.save()).expect("save encodes")
    }

    pub fn load_bytes(bytes: &[u8]) -> Result<World, String> {
        let data: SaveData = postcard::from_bytes(bytes).map_err(|e| e.to_string())?;
        Self::load(data)
    }

    pub fn load(data: SaveData) -> Result<World, String> {
        if data.version != SAVE_VERSION {
            return Err(format!("unsupported save version {}", data.version));
        }
        let mut w = World::new();
        w.tick = data.tick;
        let mut mechs = Vec::new();
        let mut programs = Vec::new();
        let mut doors = Vec::new();
        let mut locks: Vec<(BlockId, GridId, SavedLock)> = Vec::new();
        for g in &data.grids {
            w.create_grid(g.pos, g.rot, g.is_static, Some(g.id));
            let body = &mut w.phys.bodies[w.grids[&g.id].body];
            body.set_linvel(g.linvel, false);
            body.set_angvel(g.angvel, false);
            w.grids.get_mut(&g.id).unwrap().cruise = g.cruise;
            w.grids.get_mut(&g.id).unwrap().mount = g.mount;
            for b in &g.blocks {
                w.insert_block(g.id, b.cell, b.rot, b.kind, Some(b.id), Some(b.name.clone()), false);
                let blk = w.block_mut(b.id).unwrap();
                match (&b.state, &mut blk.state) {
                    (SavedState::Thruster { override_ }, BlockState::Thruster(t)) => t.override_ = *override_,
                    (SavedState::Mech { top, velocity, target, value, free }, BlockState::Mech(m)) => {
                        m.top = *top;
                        m.free = *free;
                        m.value = *value;
                        m.velocity = *velocity;
                        m.target = *target;
                        mechs.push(b.id);
                    }
                    (SavedState::Gyro { override_: o }, BlockState::Gyro { override_ }) => *override_ = *o,
                    (SavedState::Door { open }, BlockState::Door { .. }) => doors.push((b.id, *open)),
                    (SavedState::Sensor { range: r }, BlockState::Sensor { range, .. }) => *range = *r,
                    (SavedState::Gear { auto_lock, lock }, BlockState::Gear(a)) => {
                        a.auto_lock = *auto_lock;
                        if let Some(l) = lock {
                            locks.push((b.id, g.id, SavedLock { ..*l }));
                        }
                    }
                    (SavedState::Connector { auto_lock, lock, partner }, BlockState::Connector(a)) => {
                        a.auto_lock = *auto_lock;
                        a.partner = *partner;
                        if let Some(l) = lock {
                            locks.push((b.id, g.id, SavedLock { ..*l }));
                        }
                    }
                    (SavedState::Merge { enabled: e }, BlockState::Merge { enabled, status }) => {
                        *enabled = *e;
                        *status = *e as u8;
                    }
                    (SavedState::Program { source, storage }, BlockState::Program(p)) => {
                        p.storage = storage.clone();
                        programs.push((b.id, source.clone()));
                    }
                    _ => {}
                }
            }
        }
        for m in mechs {
            w.attach_joint(m);
        }
        // Restore velocities after the joints exist (multibody roots keep them there).
        for g in &data.grids {
            let body = w.grids[&g.id].body;
            if !g.is_static && g.mount.is_none() {
                w.set_body_velocity(body, Some(g.linvel), Some(g.angvel));
            }
        }
        for (block, grid, l) in locks {
            let lock = w.restore_lock(grid, l.target, pose(l.pos1, l.rot1), pose(l.pos2, l.rot2));
            if let Some(BlockState::Gear(a) | BlockState::Connector(a)) = w.block_mut(block).map(|b| &mut b.state) {
                a.lock = lock;
            }
        }
        for (d, open) in doors {
            let _ = w.set_block_prop(d, shared::protocol::BlockProp::DoorOpen(open));
        }
        for (pb, src) in programs {
            let _ = w.upload_script(pb, &src);
        }
        w.next_grid = w.next_grid.max(data.next_grid);
        w.next_block = w.next_block.max(data.next_block);
        w.saved_players = data.players.into_iter().collect();
        w.read_mech_values();
        w.events.clear();
        Ok(w)
    }
}
