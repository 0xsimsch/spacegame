//! Forces: players' jetpacks, gravity, thrusters, cockpit control and joint motors.

use crate::*;
use shared::blocks::*;
use shared::orient;
use shared::solar::gravity_at;

/// Twist angle (radians) of rotation `q` around unit `axis`.
pub fn twist_angle(q: DQuat, axis: DVec3) -> f64 {
    let v = DVec3::new(q.x, q.y, q.z);
    let mut a = 2.0 * v.dot(axis).atan2(q.w);
    if a > std::f64::consts::PI {
        a -= std::f64::consts::TAU;
    } else if a < -std::f64::consts::PI {
        a += std::f64::consts::TAU;
    }
    a
}

impl World {
    pub(crate) fn apply_player_forces(&mut self) {
        let dt = TICK_DT;
        for p in self.players.values() {
            if p.seat.is_some() {
                continue;
            }
            let body = &mut self.phys.bodies[p.body];
            body.reset_forces(false);
            let pos = body.translation();
            let vel = body.linvel();
            let g = gravity_at(pos).accel;
            let mass = body.mass();
            let inp = &p.input;
            let look = if inp.look.is_finite() { inp.look.normalize() } else { DQuat::IDENTITY };
            let mv = inp.movement.as_dvec3().clamp(DVec3::splat(-1.0), DVec3::splat(1.0));
            let mut accel = g;
            if inp.jetpack {
                // Thrust along the look frame; dampeners cancel velocity on idle axes.
                let local_vel = look.inverse() * vel;
                let mut a_local = mv * JETPACK_ACCEL;
                if inp.dampeners {
                    let g_local = look.inverse() * g;
                    for i in 0..3 {
                        if mv[i].abs() < 0.01 {
                            a_local[i] = (-local_vel[i] / dt * 0.5 - g_local[i]).clamp(-JETPACK_ACCEL, JETPACK_ACCEL);
                        }
                    }
                }
                accel += look * a_local;
            } else if p.grounded && g.length() > 0.01 {
                // Walk on the surface, tangent to gravity.
                let up = -g.normalize();
                let fwd = look * mv;
                let tangent = fwd - up * fwd.dot(up);
                let want = if tangent.length() > 0.01 { tangent.normalize() * WALK_SPEED * mv.length().min(1.0) } else { DVec3::ZERO };
                let cur = vel - up * vel.dot(up);
                let a = ((want - cur) * 10.0).clamp_length_max(40.0);
                accel += a;
                if mv.y > 0.5 {
                    body.apply_impulse(up * 5.0 * mass, true);
                }
            }
            body.add_force(accel * mass, true);
        }
    }

    pub(crate) fn apply_cockpit_control(&mut self) {
        // Reset demands.
        for g in self.grids.values_mut() {
            for b in g.blocks.values_mut() {
                if let BlockState::Thruster(t) = &mut b.state {
                    t.demand = 0.0;
                }
            }
        }
        let piloted: Vec<(GridId, IVec3, u8, PlayerInput)> = self
            .grids
            .values()
            .flat_map(|g| {
                g.blocks.values().filter_map(move |b| match b.state {
                    BlockState::Cockpit { pilot: Some(p) } => Some((g.id, b.cell, b.rot, p)),
                    _ => None,
                })
            })
            .filter_map(|(g, c, r, p)| self.players.get(&p).map(|pl| (g, c, r, pl.input.clone())))
            .collect();

        for (gid, _cell, rot, inp) in piloted {
            let g = &self.grids[&gid];
            // Static grids and joint-driven sub-grids (e.g. a gondola) aren't flown.
            if g.is_static || g.mount.is_some() {
                continue;
            }
            let body = &self.phys.bodies[g.body];
            let gp = *body.position();
            let mass = body.mass();
            let cq = orient::quat(rot);
            let to_cockpit = (gp.rotation * cq).inverse();
            let v_c = to_cockpit * body.linvel();
            let g_c = to_cockpit * gravity_at(body.center_of_mass()).accel;
            let mv = inp.movement.as_dvec3().clamp(DVec3::splat(-1.0), DVec3::splat(1.0));
            // Desired acceleration in the cockpit frame.
            let mut a_c = DVec3::ZERO;
            for i in 0..3 {
                if mv[i].abs() > 0.01 {
                    a_c[i] = mv[i] * 1e6; // saturate
                } else if inp.dampeners {
                    a_c[i] = -v_c[i] / TICK_DT * 0.3 - g_c[i];
                }
            }
            let f_grid = cq * (a_c * mass);
            // Capacity per thrust direction.
            let mut cap: HashMap<IVec3, f64> = HashMap::new();
            for b in g.blocks.values() {
                if b.kind == BlockKind::Thruster {
                    *cap.entry(orient::forward(b.rot)).or_default() += THRUSTER_MAX_FORCE;
                }
            }
            let g = self.grids.get_mut(&gid).unwrap();
            for b in g.blocks.values_mut() {
                if let BlockState::Thruster(t) = &mut b.state {
                    let d = orient::forward(b.rot);
                    let want = f_grid.dot(d.as_dvec3()).max(0.0);
                    t.demand = (want / cap[&d]).clamp(0.0, 1.0);
                }
            }
            // Built-in gyroscope: steer angular velocity toward the rotation input.
            let body_h = g.body;
            if inp.cruise != g.cruise {
                g.cruise = inp.cruise;
                self.phys.bodies[body_h].enable_ccd(inp.cruise);
            }
            let rin = inp.rotation.as_dvec3().clamp(DVec3::splat(-1.0), DVec3::splat(1.0));
            let has_rot = rin.length() > 0.01;
            if has_rot || inp.dampeners {
                let desired = gp.rotation * cq * (rin * 1.2);
                let cur = self.phys.bodies[body_h].angvel();
                self.set_body_velocity(body_h, None, Some(cur + (desired - cur) * 0.3));
            }
        }
        // Gyroscopes with an override steer the grid's angular velocity
        // (applied after the cockpit, so an autopilot script wins).
        let mut steer = Vec::new();
        for g in self.grids.values() {
            if g.is_static || g.mount.is_some() {
                continue;
            }
            let target = g.blocks.values().find_map(|b| match b.state {
                BlockState::Gyro { override_: Some(w) } => Some(w),
                _ => None,
            });
            if let Some(w) = target {
                let body = &self.phys.bodies[g.body];
                let desired = *body.rotation() * (w * std::f64::consts::PI / 180.0);
                let cur = body.angvel();
                steer.push((g.body, cur + (desired - cur) * 0.3));
            }
        }
        for (body, w) in steer {
            self.set_body_velocity(body, None, Some(w));
        }
    }

    pub(crate) fn apply_grid_forces(&mut self) {
        for g in self.grids.values_mut() {
            if g.is_static {
                continue;
            }
            let body = &mut self.phys.bodies[g.body];
            body.reset_forces(false);
            body.reset_torques(false);
            let com = body.center_of_mass();
            let grav = gravity_at(com).accel;
            let gp = *body.position();
            let mut awake = grav.length_squared() > 0.0;
            let mut force = grav * body.mass();
            let mut forces = Vec::new();
            for b in g.blocks.values_mut() {
                if let BlockState::Thruster(t) = &mut b.state {
                    let out = if t.override_ > 0.0 { t.override_ } else { t.demand };
                    t.output = out;
                    if out > 0.0 {
                        let dir = gp.rotation * orient::forward(b.rot).as_dvec3();
                        forces.push((dir * out * THRUSTER_MAX_FORCE, gp.transform_point(b.cell.as_dvec3())));
                        awake = true;
                    }
                }
            }
            if awake {
                let body = &mut self.phys.bodies[g.body];
                for (f, p) in forces {
                    body.add_force_at_point(f, p, true);
                }
                body.add_force(std::mem::take(&mut force), true);
            }
        }
    }

    pub(crate) fn update_mech_motors(&mut self) {
        let mut updates = Vec::new();
        for g in self.grids.values() {
            for b in g.blocks.values() {
                if let BlockState::Mech(m) = &b.state {
                    let Some(j) = m.joint else { continue };
                    let (axis, vel) = if b.kind == BlockKind::Piston {
                        (JointAxis::LinX, m.velocity)
                    } else if b.kind == BlockKind::Rotor {
                        // Shortest way round to the target angle.
                        let v = match m.target {
                            Some(t) => (crate::build::wrap_deg(t - m.value) * 3.0).clamp(-ROTOR_MAX_SPEED_DEG, ROTOR_MAX_SPEED_DEG),
                            None => m.velocity,
                        };
                        (JointAxis::AngX, v.to_radians())
                    } else {
                        let v = match m.target {
                            Some(t) => ((t - m.value) * 3.0).clamp(-HINGE_MAX_SPEED_DEG, HINGE_MAX_SPEED_DEG),
                            None => m.velocity,
                        };
                        (JointAxis::AngX, v.to_radians())
                    };
                    let max = match b.kind {
                        _ if m.free => 0.0,
                        BlockKind::Piston => PISTON_MAX_FORCE,
                        BlockKind::Rotor => ROTOR_MAX_TORQUE,
                        _ => HINGE_MAX_TORQUE,
                    };
                    updates.push((j, axis, if m.free { 0.0 } else { vel }, max));
                }
            }
        }
        let mut changed = Vec::new();
        for (j, axis, vel, max) in updates {
            let data = match j {
                JointRef::Impulse(h) => self.phys.impulse_joints.get_mut(h, true).map(|j| &mut j.data),
                JointRef::Multi(h) => self
                    .phys
                    .multibody_joints
                    .get_mut(h)
                    .and_then(|(mb, link)| mb.link_mut(link))
                    .map(|l| &mut l.joint.data),
            };
            let Some(data) = data else { continue };
            let (cur, cur_max) = data.motor(axis).map(|m| (m.target_vel, m.max_force)).unwrap_or((f64::NAN, f64::NAN));
            if cur != vel || cur_max != max {
                // A free joint gets no motor at all (zero stiffness, damping and force).
                data.set_motor_velocity(axis, vel, if max > 0.0 { 1000.0 } else { 0.0 });
                data.set_motor_max_force(axis, max);
                changed.push(j);
            }
        }
        // Wake both sides of joints whose motor changed.
        for g in self.grids.values() {
            for b in g.blocks.values() {
                if let BlockState::Mech(Mech { joint: Some(j), top: Some(top), .. }) = &b.state {
                    if changed.iter().any(|c| same_joint(*c, *j)) {
                        let tb = self.grids.get(top).map(|t| t.body);
                        self.phys.bodies[g.body].wake_up(true);
                        if let Some(tb) = tb {
                            self.phys.bodies[tb].wake_up(true);
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn read_mech_values(&mut self) {
        let mut vals = Vec::new();
        for g in self.grids.values() {
            let p1 = *self.phys.bodies[g.body].position();
            for b in g.blocks.values() {
                if let BlockState::Mech(m) = &b.state {
                    let Some(top) = m.top.and_then(|t| self.grids.get(&t)) else { continue };
                    let p2 = *self.phys.bodies[top.body].position();
                    let fwd = orient::forward(b.rot).as_dvec3();
                    let v = if b.kind == BlockKind::Piston {
                        let local = p1.inverse_transform_point(p2.translation);
                        (local - (b.cell.as_dvec3() + fwd)).dot(fwd)
                    } else {
                        let rel = p1.rotation.inverse() * p2.rotation;
                        let axis = if b.kind == BlockKind::Rotor { fwd } else { orient::right(b.rot).as_dvec3() };
                        twist_angle(rel, axis).to_degrees()
                    };
                    vals.push((g.id, b.cell, v));
                }
            }
        }
        for (gid, cell, v) in vals {
            if let Some(BlockState::Mech(m)) = self.grids.get_mut(&gid).and_then(|g| g.blocks.get_mut(&cell)).map(|b| &mut b.state) {
                m.value = v;
            }
        }
    }
}

fn same_joint(a: JointRef, b: JointRef) -> bool {
    match (a, b) {
        (JointRef::Multi(x), JointRef::Multi(y)) => x == y,
        (JointRef::Impulse(x), JointRef::Impulse(y)) => x == y,
        _ => false,
    }
}
