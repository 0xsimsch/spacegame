//! Client-side world model: replicated grids/players, interpolation, own-player
//! prediction and grid raycasts. Independent of Godot so it can be unit-tested.

use shared::blocks::BlockKind;
use shared::protocol::*;
use shared::solar::gravity_at;
use shared::{BlockId, DQuat, DVec3, GridId, IVec3, PlayerId, TICK_HZ};
use std::collections::{BTreeMap, HashMap, VecDeque};

/// Render this many ticks behind the newest snapshot (100 ms).
pub const INTERP_DELAY_TICKS: f64 = 6.0;
const MAX_EXTRAPOLATE_TICKS: f64 = 15.0;

#[derive(Debug, Clone)]
pub enum UiEvent {
    Connected,
    Disconnected(String),
    Chat { from: String, text: String },
    ScriptInfo(ScriptInfo),
    ScriptOutput { block: BlockId, lines: Vec<String>, error: Option<String> },
    BlockControl(BlockControlInfo),
    /// A grid's block list changed (control panel should refresh).
    GridChanged(GridId),
    OpenPanel { grid: GridId, block: Option<BlockId> },
    OpenEditor { block: BlockId },
}

#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub pos: DVec3,
    pub rot: DQuat,
}

impl Pose {
    pub fn transform(&self, p: DVec3) -> DVec3 {
        self.pos + self.rot * p
    }
    pub fn inverse_transform(&self, p: DVec3) -> DVec3 {
        self.rot.inverse() * (p - self.pos)
    }
}

pub struct ClientGrid {
    pub id: GridId,
    pub is_static: bool,
    pub blocks: HashMap<IVec3, BlockData>,
    pub by_id: HashMap<BlockId, IVec3>,
    /// Extra cells covered by multi-cell blocks, mapped to their anchor cell.
    pub occupied: HashMap<IVec3, IVec3>,
    states: VecDeque<(f64, GridState)>,
    /// Block list changed since the renderer last rebuilt this grid.
    pub dirty: bool,
}

impl ClientGrid {
    fn add_block(&mut self, b: BlockData) {
        for c in b.kind.footprint(b.cell, b.rot).into_iter().skip(1) {
            self.occupied.insert(c, b.cell);
        }
        self.by_id.insert(b.id, b.cell);
        self.blocks.insert(b.cell, b);
    }

    /// Anchor cell of the block covering `cell`, if any.
    pub fn anchor_at(&self, cell: IVec3) -> Option<IVec3> {
        if self.blocks.contains_key(&cell) { Some(cell) } else { self.occupied.get(&cell).copied() }
    }
}

pub struct ClientPlayer {
    pub id: PlayerId,
    states: VecDeque<(f64, PlayerState)>,
}

#[derive(Default)]
pub struct Prediction {
    pub valid: bool,
    pub pos: DVec3,
    pub vel: DVec3,
    pub grounded: bool,
    /// Visual offset left over from corrections; decays to zero.
    pub error: DVec3,
    history: VecDeque<(u32, PlayerInput, f64)>,
}

#[derive(Default)]
pub struct ClientState {
    pub me: Option<PlayerId>,
    pub grids: BTreeMap<GridId, ClientGrid>,
    pub players: BTreeMap<PlayerId, ClientPlayer>,
    pub names: HashMap<PlayerId, String>,
    pub values: HashMap<BlockId, f32>,
    pub controls: HashMap<BlockId, BlockControlInfo>,
    pub scripts: HashMap<BlockId, ScriptInfo>,
    pub stats: ServerStats,
    pub latest_tick: u64,
    pub render_tick: f64,
    pub removed_grids: Vec<GridId>,
    pub pred: Prediction,
    pub my_state: Option<PlayerState>,
    pub ui_events: Vec<UiEvent>,
    pub snapshots_received: u64,
}

fn lerp_state(a: &GridState, b: &GridState, t: f64) -> Pose {
    Pose { pos: a.pos.lerp(b.pos, t), rot: a.rot.slerp(b.rot, t) }
}

impl ClientState {
    pub fn apply(&mut self, msg: ServerMsg) {
        match msg {
            ServerMsg::Welcome { player, tick } => {
                self.me = Some(player);
                self.latest_tick = tick;
                self.render_tick = tick as f64 - INTERP_DELAY_TICKS;
                self.ui_events.push(UiEvent::Connected);
            }
            ServerMsg::PlayerInfo { id, name } => {
                self.names.insert(id, name);
            }
            ServerMsg::PlayerLeft { id } => {
                self.players.remove(&id);
            }
            ServerMsg::FullGrid(g) => {
                let mut cg = ClientGrid {
                    id: g.id,
                    is_static: g.is_static,
                    blocks: HashMap::new(),
                    by_id: HashMap::new(),
                    occupied: HashMap::new(),
                    states: VecDeque::new(),
                    dirty: true,
                };
                for b in g.blocks {
                    cg.add_block(b);
                }
                let st = GridState { id: g.id, pos: g.pos, rot: g.rot, linvel: Default::default(), angvel: Default::default() };
                cg.states.push_back((self.latest_tick as f64, st));
                self.grids.insert(g.id, cg);
                self.ui_events.push(UiEvent::GridChanged(g.id));
            }
            ServerMsg::GridRemoved(id) => {
                if self.grids.remove(&id).is_some() {
                    self.removed_grids.push(id);
                }
            }
            ServerMsg::BlockAdded { grid, block } => {
                if let Some(g) = self.grids.get_mut(&grid) {
                    g.add_block(block);
                    g.dirty = true;
                    self.ui_events.push(UiEvent::GridChanged(grid));
                }
            }
            ServerMsg::BlockRemoved { grid, block } => {
                if let Some(g) = self.grids.get_mut(&grid) {
                    if let Some(cell) = g.by_id.remove(&block) {
                        if let Some(b) = g.blocks.remove(&cell) {
                            for c in b.kind.footprint(cell, b.rot).into_iter().skip(1) {
                                g.occupied.remove(&c);
                            }
                        }
                        g.dirty = true;
                    }
                    self.ui_events.push(UiEvent::GridChanged(grid));
                }
            }
            ServerMsg::BlockRenamed { block, name } => {
                for g in self.grids.values_mut() {
                    if let Some(cell) = g.by_id.get(&block) {
                        g.blocks.get_mut(cell).unwrap().name = name.clone();
                        self.ui_events.push(UiEvent::GridChanged(g.id));
                    }
                }
            }
            ServerMsg::BlockControl(info) => {
                self.controls.insert(info.block, info.clone());
                self.ui_events.push(UiEvent::BlockControl(info));
            }
            ServerMsg::Snapshot(s) => self.apply_snapshot(s),
            ServerMsg::ScriptInfo(info) => {
                self.scripts.insert(info.block, info.clone());
                self.ui_events.push(UiEvent::ScriptInfo(info));
            }
            ServerMsg::ScriptOutput { block, lines, error } => {
                if let Some(s) = self.scripts.get_mut(&block) {
                    s.output.extend(lines.iter().cloned());
                    let n = s.output.len().saturating_sub(script_max_lines());
                    s.output.drain(..n);
                    s.error = error.clone();
                }
                self.ui_events.push(UiEvent::ScriptOutput { block, lines, error });
            }
            ServerMsg::Chat { from, text } => self.ui_events.push(UiEvent::Chat { from, text }),
            ServerMsg::Stats(s) => self.stats = s,
        }
    }

    fn apply_snapshot(&mut self, s: Snapshot) {
        // Unreliable channel: ignore stale snapshots.
        if s.tick + 60 < self.latest_tick {
            return;
        }
        self.snapshots_received += 1;
        self.latest_tick = self.latest_tick.max(s.tick);
        let t = s.tick as f64;
        for g in s.grids {
            if let Some(cg) = self.grids.get_mut(&g.id) {
                if cg.states.back().is_none_or(|(bt, _)| *bt < t) {
                    cg.states.push_back((t, g));
                }
                while cg.states.len() > 32 {
                    cg.states.pop_front();
                }
            }
        }
        let present: Vec<PlayerId> = s.players.iter().map(|p| p.id).collect();
        self.players.retain(|id, _| present.contains(id));
        for p in s.players {
            if Some(p.id) == self.me {
                self.reconcile(&p, s.ack_input);
                self.my_state = Some(p.clone());
            }
            let cp = self.players.entry(p.id).or_insert_with(|| ClientPlayer { id: p.id, states: VecDeque::new() });
            if cp.states.back().is_none_or(|(bt, _)| *bt < t) {
                cp.states.push_back((t, p));
            }
            while cp.states.len() > 32 {
                cp.states.pop_front();
            }
        }
        for v in s.values {
            self.values.insert(v.id, v.value);
        }
    }

    /// Advance the interpolation clock.
    pub fn advance(&mut self, dt: f64) {
        let target = self.latest_tick as f64 - INTERP_DELAY_TICKS;
        self.render_tick += dt * TICK_HZ as f64;
        let diff = target - self.render_tick;
        if diff.abs() > 30.0 {
            self.render_tick = target;
        } else {
            // Gently speed up / slow down to stay at the target delay.
            self.render_tick += diff * (dt * 2.0).min(1.0);
        }
        // Drop old states.
        let keep_after = self.render_tick - 30.0;
        for g in self.grids.values_mut() {
            while g.states.len() > 2 && g.states[1].0 < keep_after {
                g.states.pop_front();
            }
        }
        self.pred.error *= (-12.0 * dt).exp();
    }

    pub fn grid_pose(&self, id: GridId) -> Option<Pose> {
        let g = self.grids.get(&id)?;
        sample(&g.states, self.render_tick, |a, b, t| lerp_state(a, b, t), |s, dt| {
            let ang = s.angvel.as_dvec3();
            let rot = if ang.length() > 1e-9 { DQuat::from_scaled_axis(ang * dt) * s.rot } else { s.rot };
            Pose { pos: s.pos + s.linvel.as_dvec3() * dt, rot }
        })
    }

    pub fn player_state(&self, id: PlayerId) -> Option<(DVec3, DQuat, Option<BlockId>)> {
        let p = self.players.get(&id)?;
        let pos = sample(&p.states, self.render_tick, |a, b, t| Pose { pos: a.pos.lerp(b.pos, t), rot: a.look.slerp(b.look, t) }, |s, dt| {
            Pose { pos: s.pos + s.vel * dt, rot: s.look }
        })?;
        let seat = p.states.back().and_then(|(_, s)| s.seat);
        Some((pos.pos, pos.rot, seat))
    }

    pub fn block(&self, id: BlockId) -> Option<(GridId, &BlockData)> {
        self.grids.values().find_map(|g| g.by_id.get(&id).map(|c| (g.id, &g.blocks[c])))
    }

    pub fn block_world_pose(&self, id: BlockId) -> Option<Pose> {
        let (gid, b) = self.block(id)?;
        let gp = self.grid_pose(gid)?;
        Some(Pose { pos: gp.transform(b.cell.as_dvec3()), rot: gp.rot * shared::orient::quat(b.rot) })
    }

    // ------------------------------------------------------- prediction

    /// Record a local input and advance the predicted own-player position.
    pub fn predict(&mut self, input: &PlayerInput, dt: f64) {
        if !self.pred.valid {
            return;
        }
        self.pred.history.push_back((input.seq, input.clone(), dt));
        while self.pred.history.len() > 600 {
            self.pred.history.pop_front();
        }
        let (p, v) = integrate(self.pred.pos, self.pred.vel, input, dt, self.pred.grounded);
        self.pred.pos = p;
        self.pred.vel = v;
    }

    fn reconcile(&mut self, s: &PlayerState, ack: u32) {
        let seated = s.seat.is_some();
        while self.pred.history.front().is_some_and(|(seq, _, _)| *seq <= ack) {
            self.pred.history.pop_front();
        }
        if seated {
            self.pred.valid = false;
            self.pred.history.clear();
            return;
        }
        let old = self.pred.pos;
        let mut pos = s.pos;
        let mut vel = s.vel;
        for (_, inp, dt) in &self.pred.history {
            let (p, v) = integrate(pos, vel, inp, *dt, s.grounded);
            pos = p;
            vel = v;
        }
        if self.pred.valid {
            let err = old - pos;
            // Teleports: snap instead of smoothing.
            self.pred.error = if (old + self.pred.error - pos).length() > 5.0 { DVec3::ZERO } else { self.pred.error + err };
        }
        self.pred.valid = true;
        self.pred.pos = pos;
        self.pred.vel = vel;
        self.pred.grounded = s.grounded;
    }

    /// Where to draw the local player (predicted when free, interpolated when seated).
    pub fn my_render_pos(&self) -> Option<DVec3> {
        let me = self.me?;
        if self.pred.valid {
            return Some(self.pred.pos + self.pred.error);
        }
        self.player_state(me).map(|s| s.0)
    }

    pub fn my_seat(&self) -> Option<BlockId> {
        self.my_state.as_ref().and_then(|s| s.seat)
    }

    // ------------------------------------------------------- raycast

    /// First block hit along a ray (world space).
    /// `skip`: a block the ray ignores (the seat you are sitting in).
    pub fn raycast(&self, origin: DVec3, dir: DVec3, max: f64, skip: Option<BlockId>) -> Option<RayHit> {
        let mut best: Option<RayHit> = None;
        for g in self.grids.values() {
            let Some(pose) = self.grid_pose(g.id) else { continue };
            if (pose.pos - origin).length() > max + 200.0 {
                continue;
            }
            let o = pose.inverse_transform(origin);
            let d = pose.rot.inverse() * dir;
            let skip_cell = skip.and_then(|s| g.by_id.get(&s).copied());
            if let Some((cell, normal, dist)) = grid_dda(g, o, d, max, skip_cell) {
                if best.as_ref().is_none_or(|b| dist < b.dist) {
                    let blk = &g.blocks[&g.anchor_at(cell).unwrap()];
                    best = Some(RayHit { grid: g.id, cell, normal, dist, block: blk.id, kind: blk.kind });
                }
            }
        }
        best
    }
}

fn script_max_lines() -> usize {
    50
}

#[derive(Clone, Debug)]
pub struct RayHit {
    pub grid: GridId,
    pub cell: IVec3,
    pub normal: IVec3,
    pub dist: f64,
    pub block: BlockId,
    pub kind: BlockKind,
}

fn sample<S>(
    states: &VecDeque<(f64, S)>,
    t: f64,
    lerp: impl Fn(&S, &S, f64) -> Pose,
    extrapolate: impl Fn(&S, f64) -> Pose,
) -> Option<Pose> {
    let (last_t, last) = states.back()?;
    if t >= *last_t {
        let ahead = (t - last_t).min(MAX_EXTRAPOLATE_TICKS) / TICK_HZ as f64;
        return Some(extrapolate(last, ahead));
    }
    for i in (0..states.len().saturating_sub(1)).rev() {
        let (t0, s0) = &states[i];
        let (t1, s1) = &states[i + 1];
        if *t0 <= t && t <= *t1 {
            let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
            return Some(lerp(s0, s1, f));
        }
    }
    let (_, first) = states.front()?;
    Some(extrapolate(first, 0.0))
}

/// Simplified copy of the server's player movement, used for prediction.
pub fn integrate(mut pos: DVec3, mut vel: DVec3, inp: &PlayerInput, dt: f64, grounded: bool) -> (DVec3, DVec3) {
    let steps = (dt * TICK_HZ as f64).ceil().max(1.0) as usize;
    let h = dt / steps as f64;
    let look = if inp.look.is_finite() { inp.look.normalize() } else { DQuat::IDENTITY };
    let mv = inp.movement.as_dvec3().clamp(DVec3::splat(-1.0), DVec3::splat(1.0));
    for _ in 0..steps {
        let g = gravity_at(pos).accel;
        let mut accel = if grounded && !inp.jetpack { DVec3::ZERO } else { g };
        if inp.jetpack {
            let local_vel = look.inverse() * vel;
            let mut a_local = mv * sim_consts::JETPACK_ACCEL;
            if inp.dampeners {
                let g_local = look.inverse() * g;
                for i in 0..3 {
                    if mv[i].abs() < 0.01 {
                        a_local[i] = (-local_vel[i] * TICK_HZ as f64 * 0.5 - g_local[i])
                            .clamp(-sim_consts::JETPACK_ACCEL, sim_consts::JETPACK_ACCEL);
                    }
                }
            }
            accel += look * a_local;
        } else if grounded && g.length() > 0.01 {
            let up = -g.normalize();
            let fwd = look * mv;
            let tangent = fwd - up * fwd.dot(up);
            let want = if tangent.length() > 0.01 { tangent.normalize() * sim_consts::WALK_SPEED * mv.length().min(1.0) } else { DVec3::ZERO };
            let cur = vel - up * vel.dot(up);
            accel += ((want - cur) * 10.0).clamp_length_max(40.0);
        }
        vel += accel * h;
        if vel.length() > shared::SPEED_CAP {
            vel = vel.normalize() * shared::SPEED_CAP;
        }
        pos += vel * h;
    }
    (pos, vel)
}

/// Movement constants come straight from the simulation crate.
use sim as sim_consts;

/// Amanatides–Woo voxel traversal in grid-local space (cells are unit cubes
/// centred on integer coordinates). Returns (cell, entry normal, distance).
fn grid_dda(g: &ClientGrid, o: DVec3, d: DVec3, max: f64, skip: Option<IVec3>) -> Option<(IVec3, IVec3, f64)> {
    let d = d.normalize();
    let p = o + DVec3::splat(0.5);
    let mut cell = p.floor().as_ivec3();
    let step = IVec3::new(d.x.signum() as i32, d.y.signum() as i32, d.z.signum() as i32);
    let next_boundary = |c: i32, s: i32| if s > 0 { (c + 1) as f64 } else { c as f64 };
    let mut t_max = DVec3::new(
        if d.x != 0.0 { (next_boundary(cell.x, step.x) - p.x) / d.x } else { f64::INFINITY },
        if d.y != 0.0 { (next_boundary(cell.y, step.y) - p.y) / d.y } else { f64::INFINITY },
        if d.z != 0.0 { (next_boundary(cell.z, step.z) - p.z) / d.z } else { f64::INFINITY },
    );
    let t_delta = DVec3::new(
        if d.x != 0.0 { 1.0 / d.x.abs() } else { f64::INFINITY },
        if d.y != 0.0 { 1.0 / d.y.abs() } else { f64::INFINITY },
        if d.z != 0.0 { 1.0 / d.z.abs() } else { f64::INFINITY },
    );
    // Skip the ray far outside the grid's bounding box.
    let mut t = 0.0;
    let mut normal = IVec3::ZERO;
    let mut guard = 0;
    while t <= max && guard < 4096 {
        guard += 1;
        if g.anchor_at(cell).is_some_and(|a| Some(a) != skip) {
            return Some((cell, normal, t));
        }
        if t_max.x < t_max.y && t_max.x < t_max.z {
            cell.x += step.x;
            t = t_max.x;
            t_max.x += t_delta.x;
            normal = IVec3::new(-step.x, 0, 0);
        } else if t_max.y < t_max.z {
            cell.y += step.y;
            t = t_max.y;
            t_max.y += t_delta.y;
            normal = IVec3::new(0, -step.y, 0);
        } else {
            cell.z += step.z;
            t = t_max.z;
            t_max.z += t_delta.z;
            normal = IVec3::new(0, 0, -step.z);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(cell: IVec3) -> BlockData {
        BlockData { id: 1, kind: BlockKind::Armor, cell, rot: 0, name: "a".into() }
    }

    fn grid(blocks: Vec<BlockData>) -> ClientGrid {
        let mut g = ClientGrid {
            id: 1,
            is_static: true,
            blocks: HashMap::new(),
            by_id: HashMap::new(),
            occupied: HashMap::new(),
            states: VecDeque::new(),
            dirty: false,
        };
        for b in blocks {
            g.add_block(b);
        }
        g
    }

    #[test]
    fn dda_hits_large_blocks() {
        // A 2x2x2 thruster facing -Z at the origin covers x 0..1, y 0..1, z 0..1.
        let rot = shared::orient::from_axes(IVec3::NEG_Z, IVec3::Y).unwrap();
        let g = grid(vec![BlockData { id: 7, kind: BlockKind::LargeThruster, cell: IVec3::ZERO, rot, name: "L".into() }]);
        let (c, n, _) = grid_dda(&g, DVec3::new(1.0, 1.0, -5.0), DVec3::Z, 10.0, None).unwrap();
        assert_eq!((c, n), (IVec3::new(1, 1, 0), IVec3::NEG_Z));
        assert_eq!(g.anchor_at(IVec3::new(1, 1, 1)), Some(IVec3::ZERO));
        assert_eq!(g.anchor_at(IVec3::new(0, 0, -1)), None);
    }

    #[test]
    fn dda_hits_faces() {
        let blocks = grid(vec![block(IVec3::ZERO), block(IVec3::X)]);
        let (c, n, t) = grid_dda(&blocks, DVec3::new(0.0, 0.0, 5.0), DVec3::NEG_Z, 10.0, None).unwrap();
        assert_eq!((c, n), (IVec3::ZERO, IVec3::Z));
        assert!((t - 4.5).abs() < 1e-9);
        let (c, n, _) = grid_dda(&blocks, DVec3::new(5.0, 0.2, 0.1), DVec3::NEG_X, 10.0, None).unwrap();
        assert_eq!((c, n), (IVec3::X, IVec3::X));
        assert!(grid_dda(&blocks, DVec3::new(0.0, 3.0, 0.0), DVec3::Y, 10.0, None).is_none());
    }

    /// The client's movement prediction must track the authoritative simulation.
    #[test]
    fn prediction_matches_server_physics() {
        for (start, jetpack) in [(DVec3::new(0.0, 1.0e7, 0.0), true), (shared::solar::default_spawn() + (shared::solar::default_spawn() - shared::solar::BODIES[3].position).normalize() * 200.0, true)] {
            let mut w = sim::World::new();
            w.add_player(1, "p");
            w.teleport_player(1, start);
            w.step();
            let look = DQuat::from_rotation_y(0.7) * DQuat::from_rotation_x(0.3);
            let mut pos = w.player_pos(1).unwrap();
            let mut vel = DVec3::ZERO;
            let mut max_err: f64 = 0.0;
            for i in 0..120u32 {
                let mv = if i < 60 { shared::Vec3::new(0.3, 0.0, -1.0) } else { shared::Vec3::ZERO };
                let inp = PlayerInput { seq: i + 1, movement: mv, look, dampeners: true, jetpack, ..Default::default() };
                w.set_input(1, inp.clone());
                w.step();
                let (p, v) = integrate(pos, vel, &inp, shared::TICK_DT, false);
                pos = p;
                vel = v;
                max_err = max_err.max((pos - w.player_pos(1).unwrap()).length());
            }
            assert!(max_err < 0.15, "prediction drifted {max_err} m in 2 s");
        }
    }

    #[test]
    fn interpolation_between_snapshots() {
        let mut s = ClientState::default();
        s.apply(ServerMsg::Welcome { player: 1, tick: 0 });
        s.apply(ServerMsg::FullGrid(GridData { id: 7, is_static: false, pos: DVec3::ZERO, rot: DQuat::IDENTITY, blocks: vec![block(IVec3::ZERO)] }));
        for tick in [10u64, 12] {
            s.apply(ServerMsg::Snapshot(Snapshot {
                tick,
                ack_input: 0,
                players: vec![],
                grids: vec![GridState { id: 7, pos: DVec3::X * tick as f64, rot: DQuat::IDENTITY, linvel: Default::default(), angvel: Default::default() }],
                values: vec![],
            }));
        }
        s.render_tick = 11.0;
        let p = s.grid_pose(7).unwrap();
        assert!((p.pos.x - 11.0).abs() < 1e-9);
    }
}
