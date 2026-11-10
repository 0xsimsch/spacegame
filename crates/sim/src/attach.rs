//! Grids coming apart and together: grid splitting, moving blocks between
//! grids, merging, lock joints, and the blocks built on them (landing gear,
//! connectors, merge blocks).

use crate::*;
use serde::{Deserialize, Serialize};
use shared::orient;
use shared::protocol::BlockProp;

/// Depth of the landing gear's detection zone in front of its pad (m).
pub const GEAR_REACH: f64 = 0.6;
/// Connectors see each other within this distance between their faces (m)...
pub const CONNECTOR_RANGE: f64 = 2.0;
/// ...and can lock when closer than this.
pub const CONNECTOR_LOCK_DIST: f64 = 0.4;
/// Merge blocks start pulling within this distance of their target spot (m)...
pub const MERGE_CAPTURE: f64 = 2.0;
/// ...and fuse when closer than this and aligned within `MERGE_SNAP_DEG`.
pub const MERGE_SNAP: f64 = 0.15;
pub const MERGE_SNAP_DEG: f64 = 3.0;
/// Merge blocks only pull when the grids are within this angle of a common lattice.
pub const MERGE_ALIGN_DEG: f64 = 20.0;
/// After a manual unlock, auto-lock waits this many ticks.
const RELOCK_HOLD_TICKS: u64 = 180;

/// What a lock joint holds on to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LockTarget {
    Grid(GridId),
    Planet(u32),
}

/// A fixed joint holding a block's grid to something.
#[derive(Clone, Debug)]
pub struct Lock {
    pub target: LockTarget,
    pub joint: ImpulseJointHandle,
    /// Joint frames in the own body and in the target body.
    pub frame1: Pose,
    pub frame2: Pose,
}

/// State of landing gear and connectors.
#[derive(Debug, Default)]
pub struct Attach {
    pub auto_lock: bool,
    /// Held by the block that created the joint (for connectors: one side only).
    pub lock: Option<Lock>,
    /// Connectors: the connector we are joined to (both sides store it).
    pub partner: Option<BlockId>,
    /// Something lockable is in range (connectors: `candidate` is set).
    pub ready: bool,
    pub candidate: Option<BlockId>,
    /// Auto-lock waits until this tick (after a manual unlock).
    pub hold_until: u64,
}

impl Attach {
    pub fn new(auto_lock: bool) -> Self {
        Self { auto_lock, ..Default::default() }
    }

    /// 0 idle, 1 ready, 2 locked/connected.
    pub fn status(&self) -> u8 {
        if self.lock.is_some() || self.partner.is_some() {
            2
        } else if self.ready {
            1
        } else {
            0
        }
    }
}

/// Who owns a rigid body.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Owner {
    Grid(GridId),
    Planet(u32),
    Player,
}

fn owner_of(body: &RigidBody) -> Owner {
    let d = body.user_data;
    if d & PLAYER_FLAG != 0 {
        Owner::Player
    } else if d & PLANET_FLAG != 0 {
        Owner::Planet((d & 0xffff_ffff) as u32)
    } else {
        Owner::Grid(d as GridId)
    }
}

/// A connector or merge block in world space.
struct Port {
    id: BlockId,
    grid: GridId,
    group: Vec<GridId>,
    /// Centre of the block's forward face, and the forward direction.
    face: DVec3,
    fwd: DVec3,
}

impl World {
    // ------------------------------------------------------------ lock joints

    fn lock_body(&self, t: LockTarget) -> Option<RigidBodyHandle> {
        match t {
            LockTarget::Grid(g) => self.grids.get(&g).map(|g| g.body),
            LockTarget::Planet(i) => self.planet_bodies.get(i as usize).copied(),
        }
    }

    /// Fix `own` to `target` with the given joint frames.
    fn make_joint(&mut self, own: RigidBodyHandle, target: RigidBodyHandle, frame1: Pose, frame2: Pose) -> ImpulseJointHandle {
        let joint = FixedJointBuilder::new().local_frame1(frame1).local_frame2(frame2).contacts_enabled(false).build();
        let h = self.phys.insert_impulse_joint(own, target, joint);
        self.phys.bodies[own].wake_up(true);
        h
    }

    /// Lock `grid` to `target` at `anchor` (world space), keeping the current relative pose.
    fn lock_now(&mut self, grid: GridId, target: LockTarget, anchor: DVec3) -> Option<Lock> {
        let own = self.grids.get(&grid)?.body;
        let tb = self.lock_body(target)?;
        let p1 = *self.phys.bodies[own].position();
        let p2 = *self.phys.bodies[tb].position();
        let frame1 = pose(p1.inverse_transform_point(anchor), DQuat::IDENTITY);
        let frame2 = p2.inverse() * (p1 * frame1);
        let joint = self.make_joint(own, tb, frame1, frame2);
        Some(Lock { target, joint, frame1, frame2 })
    }

    /// Recreate a saved lock (after loading).
    pub(crate) fn restore_lock(&mut self, grid: GridId, target: LockTarget, frame1: Pose, frame2: Pose) -> Option<Lock> {
        let own = self.grids.get(&grid)?.body;
        let tb = self.lock_body(target)?;
        let joint = self.make_joint(own, tb, frame1, frame2);
        Some(Lock { target, joint, frame1, frame2 })
    }

    fn release(&mut self, lock: Lock) {
        if self.phys.impulse_joints.get(lock.joint).is_some() {
            self.phys.remove_impulse_joint(lock.joint);
        }
        if let Some(b) = self.lock_body(lock.target) {
            self.phys.bodies[b].wake_up(true);
        }
    }

    fn attach_mut(&mut self, block: BlockId) -> Option<&mut Attach> {
        match &mut self.block_mut(block)?.state {
            BlockState::Gear(a) | BlockState::Connector(a) => Some(a),
            _ => None,
        }
    }

    /// Drop a block's lock and, for connectors, the link on both sides.
    pub(crate) fn unlock_block(&mut self, block: BlockId) {
        let Some(a) = self.attach_mut(block) else { return };
        let lock = a.lock.take();
        let partner = a.partner.take();
        if let Some(l) = lock {
            self.release(l);
        }
        if let Some(p) = partner {
            if let Some(pa) = self.attach_mut(p) {
                let pl = pa.lock.take();
                pa.partner = None;
                if let Some(l) = pl {
                    self.release(l);
                }
            }
            self.mark_control_dirty(p);
        }
        self.mark_control_dirty(block);
    }

    /// World pose of a block (its grid's rotation times the block orientation).
    fn block_world_frame(&self, block: BlockId) -> Option<(DVec3, DQuat, DVec3)> {
        let (g, b) = self.block(block)?;
        let p = *self.phys.bodies[g.body].position();
        let centre = p.transform_point(b.cell.as_dvec3() + b.kind.center_offset(b.rot));
        let fwd = p.rotation * orient::forward(b.rot).as_dvec3();
        Some((centre, p.rotation * orient::quat(b.rot), fwd))
    }

    /// What a landing gear's pad touches: (target, anchor point on the pad).
    fn gear_probe(&self, block: BlockId) -> Option<(LockTarget, DVec3)> {
        let (gid, _) = *self.block_loc.get(&block)?;
        let (centre, rot, fwd) = self.block_world_frame(block)?;
        let group: Vec<RigidBodyHandle> = self.mech_group(gid).iter().map(|g| self.grids[g].body).collect();
        let zone = Cuboid::new(DVec3::new(0.42, 0.42, GEAR_REACH * 0.5));
        let zone_pose = pose(centre + fwd * (0.5 + GEAR_REACH * 0.5), rot);
        let bodies = &self.phys.bodies;
        let pred = |_h: ColliderHandle, co: &Collider| {
            co.is_enabled() && co.parent().is_some_and(|b| !group.contains(&b) && owner_of(&bodies[b]) != Owner::Player)
        };
        let filter = QueryFilter::default().predicate(&pred);
        let (_, co) = self.phys.intersect_shape(zone_pose, &zone, filter).next()?;
        let target = match owner_of(&bodies[co.parent()?]) {
            Owner::Grid(g) => LockTarget::Grid(g),
            Owner::Planet(i) => LockTarget::Planet(i),
            Owner::Player => return None,
        };
        Some((target, centre + fwd * 0.5))
    }

    // ------------------------------------------------------------ block props

    /// `BlockProp::Lock`: landing gear lock/unlock, connector connect/disconnect.
    pub(crate) fn set_lock(&mut self, block: BlockId, on: bool) -> Result<(), String> {
        let (gid, kind) = {
            let (g, b) = self.block(block).ok_or("no such block")?;
            (g.id, b.kind)
        };
        let tick = self.tick;
        if !on {
            self.unlock_block(block);
            if let Some(a) = self.attach_mut(block) {
                a.hold_until = tick + RELOCK_HOLD_TICKS;
            }
            return Ok(());
        }
        match kind {
            BlockKind::LandingGear => {
                if self.attach_mut(block).is_some_and(|a| a.lock.is_some()) {
                    return Ok(());
                }
                let (target, anchor) = self.gear_probe(block).ok_or("nothing to lock onto")?;
                let lock = self.lock_now(gid, target, anchor);
                self.attach_mut(block).unwrap().lock = lock;
            }
            BlockKind::Connector => {
                let a = self.attach_mut(block).unwrap();
                if a.partner.is_some() {
                    return Ok(());
                }
                let other = a.candidate.ok_or("no connector in range")?;
                let (pa, _, fa) = self.block_world_frame(block).ok_or("no such block")?;
                let (pb, _, fb) = self.block_world_frame(other).ok_or("no such block")?;
                if ((pa + fa * 0.5) - (pb + fb * 0.5)).length() > CONNECTOR_LOCK_DIST {
                    return Err("too far from the other connector".into());
                }
                let og = self.block_loc[&other].0;
                let lock = self.lock_now(gid, LockTarget::Grid(og), pa + fa * 0.5);
                let a = self.attach_mut(block).unwrap();
                a.lock = lock;
                a.partner = Some(other);
                if let Some(b) = self.attach_mut(other) {
                    b.partner = Some(block);
                }
                self.mark_control_dirty(other);
            }
            _ => return Err("only landing gear and connectors lock".into()),
        }
        self.mark_control_dirty(block);
        Ok(())
    }

    pub(crate) fn set_merge_enabled(&mut self, block: BlockId, on: bool) -> Result<(), String> {
        let gid = self.block_loc.get(&block).ok_or("no such block")?.0;
        match &mut self.block_mut(block).unwrap().state {
            BlockState::Merge { enabled, status } => {
                *enabled = on;
                *status = on as u8;
            }
            _ => return Err("not a merge block".into()),
        }
        self.mark_control_dirty(block);
        if !on {
            // A merge block switched off lets go of what it was merged with.
            self.split_grid(gid);
        }
        Ok(())
    }

    // ------------------------------------------------------------ per tick

    /// Validate locks, run landing gear auto-lock, connector and merge-block
    /// magnets, and merge grids that snapped together. Runs before the physics step.
    pub(crate) fn update_attachments(&mut self) {
        self.validate_locks();
        if self.tick % 5 == 0 {
            self.update_gears();
        }
        self.update_connectors();
        self.update_merge_blocks();
    }

    fn validate_locks(&mut self) {
        let mut broken = Vec::new();
        for g in self.grids.values() {
            for b in g.blocks.values() {
                if let BlockState::Gear(a) | BlockState::Connector(a) = &b.state {
                    let joint_gone = a.lock.as_ref().is_some_and(|l| {
                        self.phys.impulse_joints.get(l.joint).is_none() || self.lock_body(l.target).is_none()
                    });
                    let partner_gone = a.partner.is_some_and(|p| self.block(p).is_none());
                    if joint_gone || partner_gone {
                        broken.push(b.id);
                    }
                }
            }
        }
        for b in broken {
            self.unlock_block(b);
        }
    }

    fn update_gears(&mut self) {
        let gears: Vec<BlockId> = self
            .grids
            .values()
            .flat_map(|g| g.blocks.values())
            .filter(|b| matches!(&b.state, BlockState::Gear(a) if a.lock.is_none()))
            .map(|b| b.id)
            .collect();
        let tick = self.tick;
        for id in gears {
            let probe = self.gear_probe(id);
            let a = self.attach_mut(id).unwrap();
            let was = a.ready;
            a.ready = probe.is_some();
            let auto = a.auto_lock && tick >= a.hold_until;
            if was != a.ready {
                self.mark_control_dirty(id);
            }
            if let (Some((target, anchor)), true) = (probe, auto) {
                let gid = self.block_loc[&id].0;
                let lock = self.lock_now(gid, target, anchor);
                self.attach_mut(id).unwrap().lock = lock;
                self.mark_control_dirty(id);
            }
        }
    }

    fn ports(&self, kind: BlockKind) -> Vec<Port> {
        let mut out = Vec::new();
        for g in self.grids.values() {
            for b in g.blocks.values() {
                let active = match &b.state {
                    BlockState::Connector(a) => kind == BlockKind::Connector && a.partner.is_none(),
                    BlockState::Merge { enabled, .. } => kind == BlockKind::MergeBlock && *enabled && g.mount.is_none(),
                    _ => false,
                };
                if active {
                    let (c, _, f) = self.block_world_frame(b.id).unwrap();
                    out.push(Port { id: b.id, grid: g.id, group: Vec::new(), face: c + f * 0.5, fwd: f });
                }
            }
        }
        for p in &mut out {
            p.group = self.mech_group(p.grid);
        }
        out
    }

    /// Push two bodies' points toward each other like a damped spring.
    fn pull(&mut self, a: (GridId, DVec3), b: (GridId, DVec3), stiffness: f64, max_force: f64) {
        let (ba, bb) = (self.grids[&a.0].body, self.grids[&b.0].body);
        let (ra, rb) = (&self.phys.bodies[ba], &self.phys.bodies[bb]);
        let ma = if ra.is_dynamic() { ra.mass() } else { f64::INFINITY };
        let mb = if rb.is_dynamic() { rb.mass() } else { f64::INFINITY };
        if !ma.is_finite() && !mb.is_finite() {
            return;
        }
        let m_red = if !ma.is_finite() { mb } else if !mb.is_finite() { ma } else { ma * mb / (ma + mb) };
        let dv = rb.velocity_at_point(b.1) - ra.velocity_at_point(a.1);
        // Critically damped spring toward each other.
        let f = (m_red * (stiffness * (a.1 - b.1) - 2.0 * stiffness.sqrt() * dv)).clamp_length_max(max_force);
        // `f` acts on b; the opposite on a.
        if mb.is_finite() {
            self.phys.bodies[bb].add_force_at_point(f, b.1, true);
        }
        if ma.is_finite() {
            self.phys.bodies[ba].add_force_at_point(-f, a.1, true);
        }
    }

    fn update_connectors(&mut self) {
        let ports = self.ports(BlockKind::Connector);
        let mut best: HashMap<BlockId, (BlockId, f64)> = HashMap::new();
        let mut pulls = Vec::new();
        for (i, a) in ports.iter().enumerate() {
            for b in &ports[i + 1..] {
                if a.group.contains(&b.grid) || a.fwd.dot(b.fwd) > -0.7 {
                    continue;
                }
                let d = (a.face - b.face).length();
                if d > CONNECTOR_RANGE {
                    continue;
                }
                for (x, y) in [(a.id, b.id), (b.id, a.id)] {
                    if best.get(&x).is_none_or(|(_, bd)| d < *bd) {
                        best.insert(x, (y, d));
                    }
                }
                pulls.push(((a.grid, a.face), (b.grid, b.face)));
            }
        }
        // Gentle magnets between connectors in range.
        for (a, b) in pulls {
            self.pull(a, b, 4.0, 20_000.0);
        }
        let tick = self.tick;
        let mut auto = Vec::new();
        for p in &ports {
            let cand = best.get(&p.id).copied();
            let a = self.attach_mut(p.id).unwrap();
            let ready = cand.is_some_and(|(_, d)| d <= CONNECTOR_LOCK_DIST);
            let changed = a.ready != ready || a.candidate != cand.map(|c| c.0);
            a.ready = ready;
            a.candidate = cand.map(|c| c.0);
            if ready && a.auto_lock && tick >= a.hold_until {
                auto.push(p.id);
            }
            if changed {
                self.mark_control_dirty(p.id);
            }
        }
        for id in auto {
            // The partner may have connected to us earlier in this loop.
            if self.attach_mut(id).is_some_and(|a| a.partner.is_none()) {
                let _ = self.set_lock(id, true);
            }
        }
    }

    fn update_merge_blocks(&mut self) {
        // Merged status: a merge block facing another one in its own grid.
        if self.tick % 10 == 0 {
            let mut st = Vec::new();
            for g in self.grids.values() {
                for b in g.blocks.values() {
                    if let BlockState::Merge { enabled, status } = &b.state {
                        let facing = g.blocks.get(&(b.cell + orient::forward(b.rot))).is_some_and(|o| {
                            o.kind == BlockKind::MergeBlock && orient::forward(o.rot) == -orient::forward(b.rot)
                        });
                        let s = if !enabled { 0 } else if facing { 3 } else { 1 };
                        if s != *status && !(*status == 2 && s == 1) {
                            st.push((b.id, s));
                        }
                    }
                }
            }
            for (id, s) in st {
                if let Some(BlockState::Merge { status, .. }) = self.block_mut(id).map(|b| &mut b.state) {
                    *status = s;
                }
            }
        }
        let ports = self.ports(BlockKind::MergeBlock);
        let mut aligning: HashSet<BlockId> = HashSet::new();
        let mut merges: Vec<(GridId, GridId, u8, IVec3)> = Vec::new();
        let mut pulls = Vec::new();
        let mut spins = Vec::new();
        for (i, a) in ports.iter().enumerate() {
            for b in &ports[i + 1..] {
                if a.group.contains(&b.grid) || a.fwd.dot(b.fwd) > -0.9 || (a.face - b.face).length() > MERGE_CAPTURE + 0.5 {
                    continue;
                }
                let (ga, gb) = (&self.grids[&a.grid], &self.grids[&b.grid]);
                if ga.is_static && gb.is_static {
                    continue;
                }
                // The grid that stays: the static one, else the bigger one.
                let a_keeps = ga.is_static || (!gb.is_static && (ga.blocks.len(), std::cmp::Reverse(ga.id)) >= (gb.blocks.len(), std::cmp::Reverse(gb.id)));
                let (k, m) = if a_keeps { (a, b) } else { (b, a) };
                let pk = *self.phys.bodies[self.grids[&k.grid].body].position();
                let pm = *self.phys.bodies[self.grids[&m.grid].body].position();
                let (kb, mb) = (self.block(k.id).unwrap().1, self.block(m.id).unwrap().1);
                // Rotation of the merging grid in the kept grid's frame, snapped to the lattice.
                let rel = pk.rotation.inverse() * pm.rotation;
                let s = orient::nearest(rel);
                let sq = orient::quat(s);
                let angle = rel.angle_between(sq).to_degrees();
                let rot_cell = |v: IVec3| (sq * v.as_dvec3()).round().as_ivec3();
                // The two faces must touch on the lattice.
                if rot_cell(orient::forward(mb.rot)) != -orient::forward(kb.rot) || angle > MERGE_ALIGN_DEG {
                    continue;
                }
                let offset = kb.cell + orient::forward(kb.rot) - rot_cell(mb.cell);
                // Where the merging block should be, and how far off it is.
                let target = pk.transform_point(kb.cell.as_dvec3() + orient::forward(kb.rot).as_dvec3());
                let at = pm.transform_point(mb.cell.as_dvec3());
                let err = (target - at).length();
                if err > MERGE_CAPTURE {
                    continue;
                }
                aligning.insert(a.id);
                aligning.insert(b.id);
                if err < MERGE_SNAP && angle < MERGE_SNAP_DEG {
                    merges.push((k.grid, m.grid, s, offset));
                } else {
                    pulls.push(((k.grid, target), (m.grid, at)));
                    // Turn the merging grid onto the lattice.
                    let (axis, ang) = (pk.rotation * sq * pm.rotation.inverse()).to_axis_angle();
                    spins.push((k.grid, m.grid, axis * ang.clamp(-1.0, 1.0)));
                }
            }
        }
        for (a, b) in pulls {
            self.pull(a, b, 6.0, 400_000.0);
        }
        for (k, m, w) in spins {
            // Blend the relative angular velocity toward the correction.
            let (bk, bm) = (self.grids[&k].body, self.grids[&m].body);
            let wk = self.phys.bodies[bk].angvel();
            if self.phys.bodies[bm].is_dynamic() {
                let wm = self.phys.bodies[bm].angvel();
                self.set_body_velocity(bm, None, Some(wm + (wk + w * 2.0 - wm) * 0.2));
            } else {
                self.set_body_velocity(bk, None, Some(wk + (-w * 2.0 - wk) * 0.2));
            }
        }
        for p in &ports {
            let s = if aligning.contains(&p.id) { 2 } else { 1 };
            if let Some(BlockState::Merge { status, .. }) = self.block_mut(p.id).map(|b| &mut b.state) {
                if *status != 3 || s == 2 {
                    *status = s;
                }
            }
        }
        let mut done: HashSet<GridId> = HashSet::new();
        for (k, m, s, offset) in merges {
            if done.contains(&k) || done.contains(&m) || !self.grids.contains_key(&k) || !self.grids.contains_key(&m) {
                continue;
            }
            if self.merge_grids(k, m, s, offset) {
                done.insert(k);
                done.insert(m);
            }
        }
    }

    // ------------------------------------------------------------ moving blocks

    /// Move one block to another grid (or another cell), keeping its id and
    /// state. Mech joints are dropped (the caller re-attaches them) and locks
    /// are released.
    pub(crate) fn move_block(&mut self, from: GridId, cell: IVec3, to: GridId, new_cell: IVec3, new_rot: u8) {
        let g = self.grids.get_mut(&from).unwrap();
        let mut block = g.blocks.remove(&cell).unwrap();
        for c in block.kind.footprint(cell, block.rot).into_iter().skip(1) {
            g.occupied.remove(&c);
        }
        self.phys.remove_collider(block.collider);
        if let BlockState::Mech(m) = &mut block.state {
            match m.joint.take() {
                Some(JointRef::Multi(j)) => self.phys.remove_multibody_joint(j),
                Some(JointRef::Impulse(j)) => {
                    self.phys.remove_impulse_joint(j);
                }
                None => {}
            }
        }
        let had_lock = matches!(&block.state, BlockState::Gear(a) | BlockState::Connector(a) if a.lock.is_some() || a.partner.is_some());
        let enabled = !matches!(block.state, BlockState::Door { open: true });
        block.cell = new_cell;
        block.rot = new_rot;
        let body = self.grids[&to].body;
        let mut col = Self::block_collider(block.kind, new_cell, new_rot, block.id);
        col.set_enabled(enabled);
        block.collider = self.phys.insert_collider(col, Some(body));
        let id = block.id;
        let g = self.grids.get_mut(&to).unwrap();
        for c in block.kind.footprint(new_cell, new_rot).into_iter().skip(1) {
            g.occupied.insert(c, new_cell);
        }
        g.blocks.insert(new_cell, block);
        self.block_loc.insert(id, (to, new_cell));
        if had_lock {
            self.unlock_block(id);
        }
    }

    /// Fuse grid `m` into grid `k`. `s` is the orientation of `m` in `k`'s
    /// frame and `offset` where `m`'s origin cell lands. Returns false (and
    /// changes nothing) when blocks would overlap.
    pub(crate) fn merge_grids(&mut self, k: GridId, m: GridId, s: u8, offset: IVec3) -> bool {
        let sq = orient::quat(s);
        let rot_cell = |v: IVec3| (sq * v.as_dvec3()).round().as_ivec3();
        let mut moves = Vec::new();
        {
            let (gk, gm) = (&self.grids[&k], &self.grids[&m]);
            if gk.mount.is_some() || gm.mount.is_some() {
                return false;
            }
            for b in gm.blocks.values() {
                let nc = offset + rot_cell(b.cell);
                let nr = orient::nearest(sq * orient::quat(b.rot));
                if b.kind.footprint(nc, nr).iter().any(|c| gk.is_taken(*c)) {
                    return false;
                }
                moves.push((b.id, b.cell, nc, nr));
            }
        }
        moves.sort_by_key(|m| m.0);
        // Conserve momentum.
        let (bk, bm) = (self.grids[&k].body, self.grids[&m].body);
        let (rk, rm) = (&self.phys.bodies[bk], &self.phys.bodies[bm]);
        let (mk, mm) = (rk.mass(), rm.mass());
        let (lin, ang) = if rk.is_dynamic() {
            ((rk.linvel() * mk + rm.linvel() * mm) / (mk + mm), (rk.angvel() * mk + rm.angvel() * mm) / (mk + mm))
        } else {
            (DVec3::ZERO, DVec3::ZERO)
        };
        let mut mechs = Vec::new();
        for (id, cell, nc, nr) in moves {
            self.move_block(m, cell, k, nc, nr);
            if self.block(id).is_some_and(|(_, b)| b.kind.is_mech()) {
                mechs.push(id);
            }
        }
        self.delete_grid(m);
        for id in mechs {
            self.attach_joint(id);
        }
        if self.phys.bodies[bk].is_dynamic() {
            self.set_body_velocity(bk, Some(lin), Some(ang));
        }
        self.events.push(WorldEvent::GridReset { grid: k, like: k });
        true
    }

    /// Split a grid into its connected pieces. Pieces that are no longer
    /// attached become new grids; returns their ids.
    pub fn split_grid(&mut self, grid: GridId) -> Vec<GridId> {
        let Some(g) = self.grids.get(&grid) else { return Vec::new() };
        // Is the step from block `a` (anchor cell) into `cell` cut by a disabled merge block?
        let cut = |a: &Block, b: &Block, toward_b: IVec3| {
            let off = |x: &Block, d: IVec3| matches!(x.state, BlockState::Merge { enabled: false, .. }) && orient::forward(x.rot) == d;
            off(a, toward_b) || off(b, -toward_b)
        };
        let mut comp: HashMap<IVec3, usize> = HashMap::new();
        let mut comps: Vec<Vec<IVec3>> = Vec::new();
        let mut anchors: Vec<IVec3> = g.blocks.keys().copied().collect();
        anchors.sort_by_key(|c| g.blocks[c].id);
        for start in anchors {
            if comp.contains_key(&start) {
                continue;
            }
            let n = comps.len();
            let mut list = vec![start];
            comp.insert(start, n);
            let mut i = 0;
            while i < list.len() {
                let a = &g.blocks[&list[i]];
                for c in a.kind.footprint(a.cell, a.rot) {
                    for d in orient::DIRS {
                        let Some(other) = g.anchor_at(c + d) else { continue };
                        if other == a.cell || comp.contains_key(&other) || cut(a, &g.blocks[&other], d) {
                            continue;
                        }
                        comp.insert(other, n);
                        list.push(other);
                    }
                }
                i += 1;
            }
            comps.push(list);
        }
        if comps.len() < 2 {
            return Vec::new();
        }
        // The piece that keeps the grid: the one holding the head block of a
        // mounted grid, else the biggest.
        let keep = if g.mount.is_some() {
            comps.iter().position(|c| c.contains(&IVec3::ZERO)).unwrap_or(0)
        } else {
            (0..comps.len()).max_by_key(|i| (comps[*i].len(), std::cmp::Reverse(*i))).unwrap()
        };
        let body = g.body;
        let p = *self.phys.bodies[body].position();
        let (v, w, com) = {
            let b = &self.phys.bodies[body];
            (b.linvel(), b.angvel(), b.center_of_mass())
        };
        let mut new_grids = Vec::new();
        for (i, cells) in comps.into_iter().enumerate() {
            if i == keep {
                continue;
            }
            // Pieces that come off (even off a station) are free to move.
            let ng = self.create_grid(p.translation, p.rotation, false, None);
            let mut mechs = Vec::new();
            let mut centroid = DVec3::ZERO;
            for c in &cells {
                let (id, rot, kind) = {
                    let b = &self.grids[&grid].blocks[c];
                    (b.id, b.rot, b.kind)
                };
                self.move_block(grid, *c, ng, *c, rot);
                centroid += p.transform_point(c.as_dvec3());
                if kind.is_mech() {
                    mechs.push(id);
                }
            }
            for id in mechs {
                self.attach_joint(id);
            }
            // Each piece keeps the velocity its part of the grid had.
            let centroid = centroid / cells.len() as f64;
            let nb = self.grids[&ng].body;
            self.set_body_velocity(nb, Some(v + w.cross(centroid - com)), Some(w));
            self.events.push(WorldEvent::GridReset { grid: ng, like: grid });
            new_grids.push(ng);
        }
        // Mechs of the kept piece whose parent body changed keep their joints;
        // the moved ones were re-attached above.
        self.phys.bodies[body].wake_up(true);
        self.events.push(WorldEvent::GridReset { grid, like: grid });
        new_grids
    }

    /// The control-panel value of an attachable block (see `BlockValue`).
    pub fn attach_value(state: &BlockState) -> Option<f64> {
        match state {
            BlockState::Gear(a) | BlockState::Connector(a) => Some(a.status() as f64),
            BlockState::Merge { status, .. } => Some(*status as f64),
            _ => None,
        }
    }
}

/// Handle the attachment-related block properties (called from `set_block_prop`).
pub(crate) fn attach_prop(w: &mut World, block: BlockId, prop: &BlockProp) -> Option<Result<(), String>> {
    match prop {
        BlockProp::Lock(on) => Some(w.set_lock(block, *on)),
        BlockProp::AutoLock(on) => Some(match w.attach_mut(block) {
            Some(a) => {
                a.auto_lock = *on;
                w.mark_control_dirty(block);
                Ok(())
            }
            None => Err("not a landing gear or connector".into()),
        }),
        BlockProp::MergeEnabled(on) => Some(w.set_merge_enabled(block, *on)),
        _ => None,
    }
}
