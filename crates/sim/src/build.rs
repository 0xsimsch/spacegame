//! Creating and editing grids and blocks.

use crate::*;
use shared::blocks::*;
use shared::orient;
use shared::protocol::BlockProp;

impl World {
    /// Create an empty grid. Returns its id.
    pub(crate) fn create_grid(&mut self, pos: DVec3, rot: DQuat, is_static: bool, id: Option<GridId>) -> GridId {
        let id = id.unwrap_or_else(|| {
            let i = self.next_grid;
            self.next_grid += 1;
            i
        });
        self.next_grid = self.next_grid.max(id + 1);
        let builder = if is_static { RigidBodyBuilder::fixed() } else { RigidBodyBuilder::dynamic() };
        let body = builder
            .pose(pose(pos, rot))
            .user_data(id as u128)
            .linear_damping(0.0)
            .angular_damping(0.05)
            .additional_solver_iterations(2)
            .build();
        let body = self.phys.insert_body(body);
        self.grids.insert(
            id,
            Grid { id, body, is_static, blocks: HashMap::new(), cruise: false, mount: None, occupied: HashMap::new() },
        );
        self.events.push(WorldEvent::GridAdded(id));
        id
    }

    /// Insert a block into a grid without any validation. Creates the head
    /// sub-grid for pistons/hinges when `with_head` is set.
    pub(crate) fn insert_block(
        &mut self,
        grid: GridId,
        cell: IVec3,
        rot: u8,
        kind: BlockKind,
        id: Option<BlockId>,
        name: Option<String>,
        with_head: bool,
    ) -> BlockId {
        let id = id.unwrap_or_else(|| {
            let i = self.next_block;
            self.next_block += 1;
            i
        });
        self.next_block = self.next_block.max(id + 1);
        let g = self.grids.get_mut(&grid).expect("grid exists");
        let col = Self::block_collider(kind, cell, rot, id);
        let collider = self.phys.insert_collider(col, Some(g.body));
        let state = match kind {
            k if k.is_thruster() => BlockState::Thruster(Thruster { override_: 0.0, demand: 0.0, output: 0.0 }),
            k if k.is_mech() => BlockState::Mech(Mech {
                top: None,
                joint: None,
                velocity: 0.0,
                target: None,
                value: 0.0,
                free: false,
            }),
            BlockKind::Programmable => BlockState::Program(Box::new(Program {
                source: String::new(),
                host: None,
                output: VecDeque::new(),
                error: None,
                frequency: 1,
                storage: String::new(),
                pending_messages: Vec::new(),
                new_output: Vec::new(),
                output_dirty: false,
                last_run_us: 0,
            })),
            BlockKind::Cockpit => BlockState::Cockpit { pilot: None },
            BlockKind::Gyro => BlockState::Gyro { override_: None },
            BlockKind::Door => BlockState::Door { open: false },
            BlockKind::Sensor => BlockState::Sensor { range: SENSOR_DEFAULT_RANGE, detected: 0 },
            BlockKind::LandingGear => BlockState::Gear(Attach::new(true)),
            BlockKind::Connector => BlockState::Connector(Attach::new(true)),
            BlockKind::MergeBlock => BlockState::Merge { enabled: true, status: 1 },
            _ => BlockState::Plain,
        };
        let name = name.unwrap_or_else(|| self.default_name(grid, kind));
        let block = Block { id, kind, cell, rot, name, collider, state };
        let data = block.data();
        let g = self.grids.get_mut(&grid).unwrap();
        for c in kind.footprint(cell, rot).into_iter().skip(1) {
            g.occupied.insert(c, cell);
        }
        g.blocks.insert(cell, block);
        self.block_loc.insert(id, (grid, cell));
        self.events.push(WorldEvent::BlockAdded { grid, block: data });
        if with_head && kind.is_mech() {
            self.create_head(id);
        }
        id
    }

    /// The collider of one block (a box covering its footprint).
    pub(crate) fn block_collider(kind: BlockKind, cell: IVec3, rot: u8, id: BlockId) -> Collider {
        let half = BLOCK_HALF + (kind.size() - 1) as f64 * 0.5;
        ColliderBuilder::cuboid(half, half, half)
            .translation((cell.as_dvec3() + kind.center_offset(rot)) * shared::CELL_SIZE)
            .mass(kind.mass())
            .friction(0.8)
            .user_data(id as u128)
            .build()
    }

    fn default_name(&self, grid: GridId, kind: BlockKind) -> String {
        let n = self.grids[&grid].blocks.values().filter(|b| b.kind == kind).count();
        format!("{} {}", kind.display_name(), n + 1)
    }

    /// Spawn the head grid of a piston/hinge and connect it.
    fn create_head(&mut self, mech: BlockId) {
        let Some((g, b)) = self.block(mech) else { return };
        let parent_pose = *self.phys.bodies[g.body].position();
        let fwd = orient::forward(b.rot);
        let head_kind = match b.kind {
            BlockKind::Piston => BlockKind::PistonTop,
            BlockKind::Rotor => BlockKind::RotorTop,
            _ => BlockKind::HingeTop,
        };
        let rot = b.rot;
        let cell = b.cell;
        let pos = parent_pose.transform_point((cell + fwd).as_dvec3());
        let head = self.create_grid(pos, parent_pose.rotation, false, None);
        self.insert_block(head, IVec3::ZERO, rot, head_kind, None, None, false);
        self.grids.get_mut(&head).unwrap().mount = Some(mech);
        if let Some(BlockState::Mech(m)) = self.block_mut(mech).map(|b| &mut b.state) {
            m.top = Some(head);
        }
        self.attach_joint(mech);
    }

    /// (Re)create the joint between a mech block and its head grid.
    pub(crate) fn attach_joint(&mut self, mech: BlockId) {
        let Some((g, b)) = self.block(mech) else { return };
        let BlockState::Mech(m) = &b.state else { return };
        let Some(top) = m.top else { return };
        let Some(tg) = self.grids.get(&top) else { return };
        let fwd = orient::forward(b.rot).as_dvec3();
        let anchor1 = b.cell.as_dvec3() + fwd * 0.5;
        let anchor2 = -fwd * 0.5;
        let joint: GenericJoint = if b.kind == BlockKind::Piston {
            PrismaticJointBuilder::new(fwd)
                .local_anchor1(anchor1)
                .local_anchor2(anchor2)
                .limits([PISTON_MIN, PISTON_MAX])
                .motor_velocity(0.0, 1.0)
                .motor_max_force(PISTON_MAX_FORCE)
                .contacts_enabled(false)
                .build()
                .into()
        } else if b.kind == BlockKind::Rotor {
            // Spins around its forward axis, no limits.
            RevoluteJointBuilder::new(fwd)
                .local_anchor1(anchor1)
                .local_anchor2(anchor2)
                .motor_velocity(0.0, 1.0)
                .motor_max_force(ROTOR_MAX_TORQUE)
                .contacts_enabled(false)
                .build()
                .into()
        } else {
            let axis = orient::right(b.rot).as_dvec3();
            let lim = HINGE_LIMIT_DEG.to_radians();
            RevoluteJointBuilder::new(axis)
                .local_anchor1(anchor1)
                .local_anchor2(anchor2)
                .limits([-lim, lim])
                .motor_velocity(0.0, 1.0)
                .motor_max_force(HINGE_MAX_TORQUE)
                .contacts_enabled(false)
                .build()
                .into()
        };
        let (b1, b2) = (g.body, tg.body);
        let is_piston = b.kind == BlockKind::Piston;
        let initial = m.value;
        let handle = match self.phys.insert_multibody_joint(b1, b2, joint) {
            Some(h) => {
                // A new multibody joint starts at coordinate 0; restore the
                // current extension/angle (matters when loading a save).
                if initial.abs() > 1e-9 {
                    let disp = if is_piston { initial } else { initial.to_radians() };
                    let phys = &mut self.phys;
                    if let Some((mb, link)) = phys.multibody_joints.get_mut(h) {
                        if let Some(l) = mb.link_mut(link) {
                            l.joint.apply_displacement(&[disp]);
                        }
                        mb.forward_kinematics(&phys.bodies, true);
                    }
                }
                JointRef::Multi(h)
            }
            None => JointRef::Impulse(self.phys.insert_impulse_joint(b1, b2, joint)),
        };
        if let Some(BlockState::Mech(m)) = self.block_mut(mech).map(|b| &mut b.state) {
            m.joint = Some(handle);
        }
    }

    /// Remove a block without validation. Cleans up joints, seats and empty grids.
    pub(crate) fn delete_block(&mut self, grid: GridId, cell: IVec3) {
        let Some(cell) = self.grids.get(&grid).and_then(|g| g.anchor_at(cell)) else { return };
        let Some(id) = self.grids[&grid].blocks.get(&cell).map(|b| b.id) else { return };
        // Let go of whatever a landing gear or connector held.
        self.unlock_block(id);
        let g = self.grids.get_mut(&grid).unwrap();
        let Some(block) = g.blocks.remove(&cell) else { return };
        for c in block.kind.footprint(cell, block.rot).into_iter().skip(1) {
            g.occupied.remove(&c);
        }
        self.block_loc.remove(&block.id);
        self.phys.remove_collider(block.collider);
        match &block.state {
            BlockState::Mech(m) => {
                match m.joint {
                    Some(JointRef::Multi(j)) => self.phys.remove_multibody_joint(j),
                    Some(JointRef::Impulse(j)) => {
                        self.phys.remove_impulse_joint(j);
                    }
                    None => {}
                }
                if let Some(top) = m.top.and_then(|t| self.grids.get_mut(&t)) {
                    top.mount = None;
                }
            }
            BlockState::Cockpit { pilot: Some(p) } => {
                let p = *p;
                if let Some(pl) = self.players.get_mut(&p) {
                    pl.seat = None;
                }
                let pos = self.phys.bodies[self.grids[&grid].body].position().transform_point(cell.as_dvec3());
                self.release_player_body(p, pos + DVec3::Y, DVec3::ZERO);
            }
            _ => {}
        }
        self.events.push(WorldEvent::BlockRemoved { grid, block: block.id });
        let g = &self.grids[&grid];
        if g.blocks.is_empty() {
            self.delete_grid(grid);
        } else {
            self.phys.bodies[g.body].wake_up(true);
        }
    }

    pub(crate) fn delete_grid(&mut self, grid: GridId) {
        let cells: Vec<IVec3> = self.grids.get(&grid).map(|g| g.blocks.keys().copied().collect()).unwrap_or_default();
        if !cells.is_empty() {
            // Removing the last block calls back into delete_grid.
            for c in cells {
                self.delete_block(grid, c);
            }
            return;
        }
        let Some(g) = self.grids.remove(&grid) else { return };
        // Detach from the mech it hung on.
        if let Some(m) = g.mount {
            if let Some(BlockState::Mech(mech)) = self.block_mut(m).map(|b| &mut b.state) {
                mech.top = None;
                mech.joint = None;
            }
        }
        self.phys.remove_body(g.body);
        self.events.push(WorldEvent::GridRemoved(grid));
    }

    fn check_reach(&self, player: PlayerId, target: DVec3) -> Result<(), String> {
        let p = self.player_pos(player).ok_or("no such player")?;
        if (p - target).length() > INTERACT_RANGE + 3.0 {
            return Err("too far away".into());
        }
        Ok(())
    }

    // ------------------------------------------------------- player commands

    pub fn place_block(&mut self, player: PlayerId, grid: GridId, cell: IVec3, rot: u8, kind: BlockKind) -> Result<BlockId, String> {
        if !kind.buildable() {
            return Err("that block cannot be built".into());
        }
        let g = self.grids.get(&grid).ok_or("no such grid")?;
        let rot = rot % orient::ORIENTATION_COUNT;
        let cells = kind.footprint(cell, rot);
        if cells.iter().any(|c| g.is_taken(*c)) {
            return Err("cell is occupied".into());
        }
        let neighbour = cells.iter().any(|c| orient::DIRS.iter().any(|d| g.is_taken(*c + *d)));
        if !neighbour {
            return Err("blocks must touch the grid".into());
        }
        if kind.is_mech() {
            // The head would overlap a block.
            if g.is_taken(cell + orient::forward(rot)) {
                return Err("no room for the head".into());
            }
        }
        let wp = self.phys.bodies[g.body].position().transform_point(cell.as_dvec3());
        self.check_reach(player, wp)?;
        Ok(self.insert_block(grid, cell, rot, kind, None, None, true))
    }

    pub fn remove_block(&mut self, player: PlayerId, grid: GridId, cell: IVec3) -> Result<(), String> {
        let g = self.grids.get(&grid).ok_or("no such grid")?;
        if !g.is_taken(cell) {
            return Err("no block there".into());
        }
        let wp = self.phys.bodies[g.body].position().transform_point(cell.as_dvec3());
        self.check_reach(player, wp)?;
        self.delete_block(grid, cell);
        // The rest may have fallen into separate pieces.
        if self.grids.contains_key(&grid) {
            self.split_grid(grid);
        }
        Ok(())
    }

    pub fn new_grid(&mut self, player: PlayerId, pos: DVec3, rot: DQuat, kind: BlockKind, is_static: bool) -> Result<GridId, String> {
        if !kind.buildable() {
            return Err("that block cannot be built".into());
        }
        self.check_reach(player, pos)?;
        let rot = if rot.is_finite() { rot.normalize() } else { DQuat::IDENTITY };
        let g = self.create_grid(pos, rot, is_static, None);
        self.insert_block(g, IVec3::ZERO, orient::from_axes(IVec3::NEG_Z, IVec3::Y).unwrap(), kind, None, None, true);
        Ok(g)
    }

    pub fn rename_block(&mut self, block: BlockId, name: &str) -> Result<(), String> {
        let name: String = name.trim().chars().take(48).collect();
        if name.is_empty() {
            return Err("name cannot be empty".into());
        }
        let (gid, _) = *self.block_loc.get(&block).ok_or("no such block")?;
        self.block_mut(block).unwrap().name = name.clone();
        self.events.push(WorldEvent::BlockRenamed { grid: gid, block, name });
        Ok(())
    }

    pub fn set_block_prop(&mut self, block: BlockId, prop: BlockProp) -> Result<(), String> {
        if let Some(r) = crate::attach::attach_prop(self, block, &prop) {
            return r;
        }
        let b = self.block_mut(block).ok_or("no such block")?;
        match (&mut b.state, prop) {
            (BlockState::Thruster(t), BlockProp::ThrusterOverride(v)) => t.override_ = (v as f64).clamp(0.0, 1.0),
            (BlockState::Mech(m), BlockProp::PistonVelocity(v)) if b.kind == BlockKind::Piston => {
                m.velocity = (v as f64).clamp(-PISTON_MAX_SPEED, PISTON_MAX_SPEED)
            }
            (BlockState::Mech(m), BlockProp::HingeVelocity(v)) if b.kind.is_angular() => {
                let max = if b.kind == BlockKind::Rotor { ROTOR_MAX_SPEED_DEG } else { HINGE_MAX_SPEED_DEG };
                m.velocity = (v as f64).clamp(-max, max);
                m.target = None;
            }
            (BlockState::Mech(m), BlockProp::HingeTarget(t)) if b.kind == BlockKind::Hinge => {
                m.target = t.map(|t| (t as f64).clamp(-HINGE_LIMIT_DEG, HINGE_LIMIT_DEG))
            }
            (BlockState::Mech(m), BlockProp::HingeTarget(t)) if b.kind == BlockKind::Rotor => {
                m.target = t.map(|t| wrap_deg(t as f64))
            }
            (BlockState::Mech(m), BlockProp::Free(on)) => m.free = on,
            (BlockState::Gyro { override_ }, BlockProp::GyroOverride(v)) => {
                *override_ = v.map(|a| DVec3::from(a.map(|x| x as f64)).clamp(DVec3::splat(-GYRO_MAX_RATE_DEG), DVec3::splat(GYRO_MAX_RATE_DEG)))
            }
            (BlockState::Door { open }, BlockProp::DoorOpen(on)) => {
                *open = on;
                let col = b.collider;
                // An open door has no collider, so players and blocks pass through.
                self.phys.colliders[col].set_enabled(!on);
            }
            (BlockState::Sensor { range, .. }, BlockProp::SensorRange(r)) if r.is_finite() => {
                *range = (r as f64).clamp(1.0, SENSOR_MAX_RANGE)
            }
            (BlockState::Program(p), BlockProp::ScriptFrequency(f)) if [1, 10, 100].contains(&f) => p.frequency = f,
            _ => return Err("property does not apply to this block".into()),
        }
        self.mark_control_dirty(block);
        Ok(())
    }

    // ---------------------------------------------------------------- seats

    pub fn enter_seat(&mut self, player: PlayerId, block: BlockId) -> Result<(), String> {
        let pos = self.block_world_pos(block).ok_or("no such block")?;
        self.check_reach(player, pos)?;
        self.enter_seat_unchecked(player, block)
    }

    /// Nearest free cockpit within `range` metres of the player.
    pub fn nearest_free_seat(&self, player: PlayerId, range: f64, name: Option<&str>) -> Option<BlockId> {
        let p = self.player_pos(player)?;
        self.grids
            .values()
            .flat_map(|g| g.blocks.values())
            .filter(|b| matches!(b.state, BlockState::Cockpit { pilot: None }))
            .filter(|b| name.is_none_or(|n| b.name.eq_ignore_ascii_case(n)))
            .filter_map(|b| self.block_world_pos(b.id).map(|w| (b.id, (w - p).length())))
            .filter(|(_, d)| *d <= range)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    pub fn enter_seat_unchecked(&mut self, player: PlayerId, block: BlockId) -> Result<(), String> {
        if self.players.get(&player).and_then(|p| p.seat).is_some() {
            return Err("already seated".into());
        }
        let b = self.block_mut(block).ok_or("no such block")?;
        match &mut b.state {
            BlockState::Cockpit { pilot: pilot @ None } => *pilot = Some(player),
            BlockState::Cockpit { .. } => return Err("seat is taken".into()),
            _ => return Err("not a seat".into()),
        }
        let p = self.players.get_mut(&player).unwrap();
        p.seat = Some(block);
        let (body, col) = (p.body, p.collider);
        self.phys.colliders[col].set_enabled(false);
        self.phys.bodies[body].set_enabled(false);
        Ok(())
    }

    pub fn exit_seat(&mut self, player: PlayerId) {
        let Some(seat) = self.players.get_mut(&player).and_then(|p| p.seat.take()) else { return };
        let mut exit = None;
        if let Some((g, b)) = self.block(seat) {
            let gp = *self.phys.bodies[g.body].position();
            let up = gp.rotation * orient::up(b.rot).as_dvec3();
            exit = Some((gp.transform_point(b.cell.as_dvec3()) + up * 1.5, self.phys.bodies[g.body].linvel()));
        }
        if let Some(BlockState::Cockpit { pilot }) = self.block_mut(seat).map(|b| &mut b.state) {
            *pilot = None;
        }
        let (pos, vel) = exit.unwrap_or((self.player_pos(player).unwrap_or_default(), DVec3::ZERO));
        self.release_player_body(player, pos, vel);
    }

    fn release_player_body(&mut self, player: PlayerId, pos: DVec3, vel: DVec3) {
        let Some(p) = self.players.get(&player) else { return };
        let (body, col) = (p.body, p.collider);
        let b = &mut self.phys.bodies[body];
        b.set_enabled(true);
        b.set_translation(pos, true);
        b.set_linvel(vel, true);
        self.phys.colliders[col].set_enabled(true);
    }

    pub(crate) fn update_seated_players(&mut self) {
        let seated: Vec<(PlayerId, BlockId)> =
            self.players.values().filter_map(|p| p.seat.map(|s| (p.id, s))).collect();
        for (pid, seat) in seated {
            let Some(pos) = self.block_world_pos(seat) else {
                self.exit_seat(pid);
                continue;
            };
            let (gid, _) = self.block_loc[&seat];
            let vel = self.phys.bodies[self.grids[&gid].body].linvel();
            let body = self.players[&pid].body;
            let b = &mut self.phys.bodies[body];
            b.set_translation(pos, false);
            b.set_linvel(vel, false);
        }
    }

    /// Move a whole mechanical group so that `grid` ends up at `pos` (keeps relative poses).
    pub fn teleport_group(&mut self, grid: GridId, pos: DVec3) {
        let Some(origin) = self.grid_pose(grid).map(|p| p.translation) else { return };
        let delta = pos - origin;
        let bodies: Vec<RigidBodyHandle> = self.mech_group(grid).iter().map(|g| self.grids[g].body).collect();
        for &body in &bodies {
            let b = &mut self.phys.bodies[body];
            let t = b.translation();
            b.set_translation(t + delta, true);
            self.set_body_velocity(body, Some(DVec3::ZERO), Some(DVec3::ZERO));
        }
        for &body in &bodies {
            self.sync_multibody_from_root(body);
        }
        self.update_seated_players();
    }
}

/// Wrap an angle in degrees to (-180, 180].
pub fn wrap_deg(a: f64) -> f64 {
    let mut a = a % 360.0;
    if a > 180.0 {
        a -= 360.0;
    } else if a <= -180.0 {
        a += 360.0;
    }
    a
}
