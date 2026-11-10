//! Ready-made constructions for testing and demos (`/spawn <name>`).

use crate::*;
use shared::BlockId;
use shared::examples;
use shared::orient::from_axes;

pub const NAMES: [&str; 8] = ["ship", "arm", "pad", "elevator", "crane", "robot", "bridge", "carousel"];

fn rot(fwd: IVec3, up: IVec3) -> u8 {
    from_axes(fwd, up).expect("valid axes")
}

impl World {
    /// Spawn a prefab with its origin at `pos`, oriented by `orientation`.
    /// Returns the main grid id.
    pub fn spawn_prefab(&mut self, name: &str, pos: DVec3, orientation: DQuat, script: Option<&str>) -> Option<GridId> {
        match name {
            "ship" => Some(self.spawn_ship(pos, orientation, script)),
            "arm" => Some(self.spawn_arm(pos, orientation)),
            "pad" => Some(self.spawn_pad(pos, orientation)),
            "elevator" => Some(self.spawn_elevator(pos, orientation)),
            "crane" => Some(self.spawn_crane(pos, orientation)),
            "robot" => Some(self.spawn_robot(pos, orientation)),
            "bridge" => Some(self.spawn_bridge(pos, orientation)),
            "carousel" => Some(self.spawn_carousel(pos, orientation)),
            _ => None,
        }
    }

    /// A small symmetric ship with thrusters in all six directions, a cockpit
    /// and a programmable block. Centre of mass is at the cockpit.
    fn spawn_ship(&mut self, pos: DVec3, orientation: DQuat, script: Option<&str>) -> GridId {
        let g = self.create_grid(pos, orientation, false, None);
        let ident = rot(IVec3::NEG_Z, IVec3::Y);
        let b = |w: &mut World, cell: IVec3, r: u8, k: BlockKind, name: Option<&str>| {
            w.insert_block(g, cell, r, k, None, name.map(String::from), false)
        };
        b(self, IVec3::ZERO, ident, BlockKind::Cockpit, None);
        let pb = b(self, IVec3::Y, ident, BlockKind::Programmable, Some("Flight Computer"));
        b(self, IVec3::NEG_Y, rot(IVec3::NEG_Y, IVec3::Z), BlockKind::Thruster, Some("Down Thruster"));
        // Lifters on the four corners.
        for (i, (x, z)) in [(1, 1), (-1, 1), (1, -1), (-1, -1)].into_iter().enumerate() {
            b(self, IVec3::new(x, 0, z), rot(IVec3::Y, IVec3::Z), BlockKind::Thruster, Some(&format!("Lift Thruster {}", i + 1)));
        }
        // Ring around the cockpit.
        b(self, IVec3::new(0, 0, 1), rot(IVec3::NEG_Z, IVec3::Y), BlockKind::Thruster, Some("Main Thruster"));
        b(self, IVec3::new(0, 0, -1), rot(IVec3::Z, IVec3::Y), BlockKind::Thruster, Some("Brake Thruster"));
        b(self, IVec3::new(1, 0, 0), rot(IVec3::NEG_X, IVec3::Y), BlockKind::Thruster, Some("Left Thruster"));
        b(self, IVec3::new(-1, 0, 0), rot(IVec3::X, IVec3::Y), BlockKind::Thruster, Some("Right Thruster"));
        if let Some(src) = script {
            let _ = self.upload_script(pb, src);
        }
        g
    }

    /// A static base with a piston, a hinge on the piston head and an arm on the hinge.
    fn spawn_arm(&mut self, pos: DVec3, orientation: DQuat) -> GridId {
        let g = self.create_grid(pos, orientation, true, None);
        let ident = rot(IVec3::NEG_Z, IVec3::Y);
        for x in -1..=1 {
            for z in -1..=1 {
                if (x, z) != (0, 0) {
                    self.insert_block(g, IVec3::new(x, 0, z), ident, BlockKind::Armor, None, None, false);
                }
            }
        }
        let pb = self.insert_block(g, IVec3::new(2, 0, 0), ident, BlockKind::Programmable, None, Some("Arm Controller".into()), false);
        let piston = self.insert_block(g, IVec3::ZERO, rot(IVec3::Y, IVec3::Z), BlockKind::Piston, None, Some("Arm Piston".into()), true);
        let head = match &self.block(piston).unwrap().1.state {
            BlockState::Mech(m) => m.top.unwrap(),
            _ => unreachable!(),
        };
        let hinge = self.insert_block(head, IVec3::Y, rot(IVec3::Y, IVec3::Z), BlockKind::Hinge, None, Some("Elbow Hinge".into()), true);
        let arm = match &self.block(hinge).unwrap().1.state {
            BlockState::Mech(m) => m.top.unwrap(),
            _ => unreachable!(),
        };
        for y in 1..=3 {
            self.insert_block(arm, IVec3::new(0, y, 0), ident, BlockKind::Armor, None, None, false);
        }
        let _ = self.upload_script(pb, examples::ARM_SWEEP);
        g
    }

    /// The head sub-grid created for a piston/hinge.
    fn head_of(&self, mech: BlockId) -> GridId {
        match &self.block(mech).expect("mech exists").1.state {
            BlockState::Mech(m) => m.top.expect("mech has a head"),
            _ => panic!("not a mech block"),
        }
    }

    fn armor(&mut self, g: GridId, cells: impl IntoIterator<Item = IVec3>) {
        let ident = rot(IVec3::NEG_Z, IVec3::Y);
        for c in cells {
            self.insert_block(g, c, ident, BlockKind::Armor, None, None, false);
        }
    }

    fn mech(&mut self, g: GridId, cell: IVec3, fwd: IVec3, up: IVec3, kind: BlockKind, name: &str) -> (BlockId, GridId) {
        let id = self.insert_block(g, cell, rot(fwd, up), kind, None, Some(name.into()), true);
        (id, self.head_of(id))
    }

    fn base_with_controller(&mut self, pos: DVec3, orientation: DQuat, half: i32, name: &str, script: &str) -> GridId {
        let g = self.create_grid(pos, orientation, true, None);
        self.armor(g, (-half..=half).flat_map(|x| (-half..=half).map(move |z| IVec3::new(x, 0, z))));
        let ident = rot(IVec3::NEG_Z, IVec3::Y);
        let pb = self.insert_block(g, IVec3::new(half + 1, 0, 0), ident, BlockKind::Programmable, None, Some(name.into()), false);
        // Scripts run after the whole construct exists (see `finish_controller`).
        let _ = script;
        let _ = pb;
        g
    }

    fn finish_controller(&mut self, base: GridId, script: &str) {
        let pb = self.grids[&base].blocks.values().find(|b| b.kind == BlockKind::Programmable).map(|b| b.id);
        if let Some(pb) = pb {
            let _ = self.upload_script(pb, script);
        }
    }

    /// Three pistons stacked on top of each other carrying a platform (15 m of travel).
    fn spawn_elevator(&mut self, pos: DVec3, orientation: DQuat) -> GridId {
        let g = self.base_with_controller(pos, orientation, 1, "Elevator Controller", examples::ELEVATOR);
        let (_, h1) = self.mech(g, IVec3::Y, IVec3::Y, IVec3::Z, BlockKind::Piston, "Lift 1");
        let (_, h2) = self.mech(h1, IVec3::Y, IVec3::Y, IVec3::Z, BlockKind::Piston, "Lift 2");
        let (_, h3) = self.mech(h2, IVec3::Y, IVec3::Y, IVec3::Z, BlockKind::Piston, "Lift 3");
        self.armor(h3, (-1..=1).flat_map(|x| (-1..=1).map(move |z| IVec3::new(x, 1, z))));
        self.armor(h3, [IVec3::new(-1, 2, -1), IVec3::new(1, 2, -1), IVec3::new(-1, 2, 1), IVec3::new(1, 2, 1)]);
        self.finish_controller(g, examples::ELEVATOR);
        g
    }

    /// A tower with a luffing, telescoping boom and a self-levelling hook.
    fn spawn_crane(&mut self, pos: DVec3, orientation: DQuat) -> GridId {
        let g = self.base_with_controller(pos, orientation, 1, "Crane Controller", examples::CRANE);
        // The whole tower turns on a rotor.
        let (_, tower) = self.mech(g, IVec3::Y, IVec3::Y, IVec3::Z, BlockKind::Rotor, "Slew Rotor");
        self.armor(tower, (1..=3).map(|y| IVec3::new(0, y, 0)));
        let (_, boom) = self.mech(tower, IVec3::new(0, 4, 0), IVec3::Y, IVec3::Z, BlockKind::Hinge, "Boom Hinge");
        // Boom along +Z from the hinge head, then a telescoping section.
        self.armor(boom, [IVec3::new(0, 0, 1), IVec3::new(0, 0, 2)]);
        let (_, tele) = self.mech(boom, IVec3::new(0, 0, 3), IVec3::Z, IVec3::Y, BlockKind::Piston, "Boom Piston");
        self.armor(tele, [IVec3::new(0, 0, 1)]);
        let (_, hook) = self.mech(tele, IVec3::new(0, -1, 1), IVec3::NEG_Y, IVec3::Z, BlockKind::Hinge, "Hook Hinge");
        self.armor(hook, [IVec3::new(0, -1, 0), IVec3::new(0, -2, 0), IVec3::new(1, -2, 0), IVec3::new(-1, -2, 0)]);
        self.finish_controller(g, examples::CRANE);
        g
    }

    /// Shoulder hinge -> telescoping upper arm -> elbow (perpendicular axis) -> wrist -> gripper.
    fn spawn_robot(&mut self, pos: DVec3, orientation: DQuat) -> GridId {
        let g = self.base_with_controller(pos, orientation, 1, "Robot Controller", examples::ROBOT);
        let (_, upper) = self.mech(g, IVec3::Y, IVec3::Y, IVec3::Z, BlockKind::Hinge, "Shoulder");
        let (_, reach) = self.mech(upper, IVec3::Y, IVec3::Y, IVec3::Z, BlockKind::Piston, "Reach Piston");
        // Elbow with up = X turns around Z, perpendicular to the shoulder.
        let (_, fore) = self.mech(reach, IVec3::Y, IVec3::Y, IVec3::X, BlockKind::Hinge, "Elbow");
        self.armor(fore, [IVec3::new(0, 1, 0), IVec3::new(0, 2, 0)]);
        let (_, hand) = self.mech(fore, IVec3::new(0, 3, 0), IVec3::Y, IVec3::Z, BlockKind::Hinge, "Wrist");
        self.armor(hand, [IVec3::new(0, 1, 0), IVec3::new(-1, 1, 0), IVec3::new(1, 1, 0), IVec3::new(-1, 2, 0), IVec3::new(1, 2, 0)]);
        self.finish_controller(g, examples::ROBOT);
        g
    }

    /// A wall with a 3x6 drawbridge on a hinge.
    fn spawn_bridge(&mut self, pos: DVec3, orientation: DQuat) -> GridId {
        let g = self.create_grid(pos, orientation, true, None);
        self.armor(g, (-2..=2).flat_map(|x| (0..=2).map(move |y| IVec3::new(x, y, 0))));
        self.armor(g, [IVec3::new(-2, 3, 0), IVec3::new(2, 3, 0), IVec3::new(-1, 3, 0), IVec3::new(1, 3, 0)]);
        let ident = rot(IVec3::NEG_Z, IVec3::Y);
        self.insert_block(g, IVec3::new(3, 0, 0), ident, BlockKind::Programmable, None, Some("Bridge Controller".into()), false);
        let (_, plate) = self.mech(g, IVec3::new(0, 3, 0), IVec3::Z, IVec3::Y, BlockKind::Hinge, "Bridge Hinge");
        self.armor(plate, (-1..=1).flat_map(|x| (0..6).map(move |z| IVec3::new(x, 0, z))).filter(|c| *c != IVec3::ZERO));
        self.finish_controller(g, examples::BRIDGE);
        g
    }

    /// A rotor-driven carousel with four swinging gondolas (each with a seat).
    fn spawn_carousel(&mut self, pos: DVec3, orientation: DQuat) -> GridId {
        let g = self.base_with_controller(pos, orientation, 2, "Carousel Controller", examples::CAROUSEL);
        let (_, hub) = self.mech(g, IVec3::Y, IVec3::Y, IVec3::Z, BlockKind::Rotor, "Carousel Rotor");
        self.armor(hub, (1..=4).map(|y| IVec3::new(0, y, 0)));
        let dirs = [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z];
        for d in dirs {
            self.armor(hub, (1..=4).map(|r| d * r + IVec3::new(0, 4, 0)));
        }
        let ident = rot(IVec3::NEG_Z, IVec3::Y);
        for (i, d) in dirs.into_iter().enumerate() {
            // Hinge under each arm tip; with up = radial direction its axis is
            // tangential, so positive angles swing the gondola outwards.
            let (_, gondola) = self.mech(hub, d * 4 + IVec3::new(0, 3, 0), IVec3::NEG_Y, d, BlockKind::Hinge, &format!("Swing {}", i + 1));
            self.armor(gondola, [IVec3::new(0, -1, 0)]);
            self.insert_block(gondola, IVec3::new(0, -2, 0), ident, BlockKind::Cockpit, None, Some(format!("Gondola Seat {}", i + 1)), false);
        }
        self.finish_controller(g, examples::CAROUSEL);
        g
    }

    /// A flat static 7x7 landing pad.
    fn spawn_pad(&mut self, pos: DVec3, orientation: DQuat) -> GridId {
        let g = self.create_grid(pos, orientation, true, None);
        let ident = rot(IVec3::NEG_Z, IVec3::Y);
        for x in -3..=3 {
            for z in -3..=3 {
                self.insert_block(g, IVec3::new(x, 0, z), ident, BlockKind::Armor, None, None, false);
            }
        }
        g
    }
}

/// Orientation whose up (+Y) points away from the dominant body at `pos`.
pub fn surface_orientation(pos: DVec3) -> DQuat {
    let g = shared::solar::gravity_at(pos).accel;
    if g.length() < 1e-6 {
        return DQuat::IDENTITY;
    }
    DQuat::from_rotation_arc(DVec3::Y, -g.normalize())
}
