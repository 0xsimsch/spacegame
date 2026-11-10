//! Ready-made constructions for testing and demos (`/spawn <name>`).

use crate::*;
use shared::examples;
use shared::orient::from_axes;

pub const NAMES: [&str; 3] = ["ship", "arm", "pad"];

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
