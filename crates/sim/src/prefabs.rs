//! Ready-made constructions for testing and demos (`/spawn <name>`).

use crate::*;
use shared::BlockId;
use shared::examples;
use shared::orient::from_axes;

pub const NAMES: [&str; 11] =
    ["ship", "arm", "pad", "elevator", "crane", "robot", "bridge", "carousel", "shuttle", "drone", "frigate"];

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
            "shuttle" => Some(self.spawn_shuttle(pos, orientation)),
            "drone" => Some(self.spawn_drone(pos, orientation)),
            "frigate" => Some(self.spawn_frigate(pos, orientation)),
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
        // Cockpit on top (clear view forward), flight computer in the middle;
        // both weigh the same, so the centre of mass stays on the thrust axes.
        b(self, IVec3::Y, ident, BlockKind::Cockpit, None);
        let pb = b(self, IVec3::ZERO, ident, BlockKind::Programmable, Some("Flight Computer"));
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

    /// A walk-in shuttle: 5x4x7 hull, cockpit at the front, side door, rear
    /// loading ramp on a hinge, four piston landing legs, thrusters on all six
    /// sides, a gyroscope and a flight computer running the autopilot.
    ///
    /// Grid frame: -Z forward, +Y up. The origin is the floor centre; the leg
    /// feet reach 3.5 m below it, so spawn it 3.6 m above the ground.
    fn spawn_shuttle(&mut self, pos: DVec3, orientation: DQuat) -> GridId {
        let g = self.create_grid(pos, orientation, false, None);
        let ident = rot(IVec3::NEG_Z, IVec3::Y);
        let mut hull = Vec::new();
        for x in -2..=2 {
            for z in -3..=3 {
                if !(x == 0 && z == 3) {
                    hull.push(IVec3::new(x, 0, z)); // floor (the ramp hinge sits at 0,0,3)
                }
                hull.push(IVec3::new(x, 3, z)); // roof
            }
        }
        for z in -3..=3 {
            for y in 1..=2 {
                hull.push(IVec3::new(2, y, z));
                if z != 0 {
                    hull.push(IVec3::new(-2, y, z)); // left wall, door at z = 0
                }
            }
        }
        // Front wall around the cockpit window.
        hull.extend([IVec3::new(-1, 1, -3), IVec3::new(1, 1, -3), IVec3::new(-1, 2, -3), IVec3::new(0, 2, -3), IVec3::new(1, 2, -3)]);
        self.armor(g, hull);
        let b = |w: &mut World, cell: IVec3, r: u8, k: BlockKind, name: &str| {
            w.insert_block(g, cell, r, k, None, Some(name.to_string()), false)
        };
        b(self, IVec3::new(0, 1, -3), ident, BlockKind::Cockpit, "Shuttle Cockpit");
        b(self, IVec3::new(1, 1, -2), ident, BlockKind::Programmable, "Shuttle Computer");
        b(self, IVec3::new(-1, 1, -2), ident, BlockKind::Gyro, "Gyroscope");
        // Side door: two stacked door blocks facing outwards (-X).
        let door_rot = rot(IVec3::NEG_X, IVec3::Y);
        b(self, IVec3::new(-2, 1, 0), door_rot, BlockKind::Door, "Side Door Lower");
        b(self, IVec3::new(-2, 2, 0), door_rot, BlockKind::Door, "Side Door Upper");
        // Thrusters (forward = direction they push the ship).
        let lift = rot(IVec3::Y, IVec3::Z);
        let mut i = 0;
        for x in [-1, 1] {
            for z in [-3, -1, 1, 3] {
                i += 1;
                b(self, IVec3::new(x, -1, z), lift, BlockKind::Thruster, &format!("Lift Thruster {i}"));
            }
        }
        let down = rot(IVec3::NEG_Y, IVec3::Z);
        b(self, IVec3::new(-1, 4, 0), down, BlockKind::Thruster, "Down Thruster 1");
        b(self, IVec3::new(1, 4, 0), down, BlockKind::Thruster, "Down Thruster 2");
        let main = rot(IVec3::NEG_Z, IVec3::Y);
        for (n, c) in [(-2, 1), (-2, 2), (2, 1), (2, 2)].into_iter().enumerate() {
            b(self, IVec3::new(c.0, c.1, 4), main, BlockKind::Thruster, &format!("Main Thruster {}", n + 1));
        }
        let brake = rot(IVec3::Z, IVec3::Y);
        b(self, IVec3::new(-2, 1, -4), brake, BlockKind::Thruster, "Brake Thruster 1");
        b(self, IVec3::new(2, 1, -4), brake, BlockKind::Thruster, "Brake Thruster 2");
        for z in [-2, 2] {
            b(self, IVec3::new(3, 1, z), rot(IVec3::NEG_X, IVec3::Y), BlockKind::Thruster, &format!("Left Thruster {}", if z < 0 { 1 } else { 2 }));
            b(self, IVec3::new(-3, 1, z), rot(IVec3::X, IVec3::Y), BlockKind::Thruster, &format!("Right Thruster {}", if z < 0 { 1 } else { 2 }));
        }
        // Landing legs: pistons pointing down with a foot block.
        let mut n = 0;
        for x in [-2, 2] {
            for z in [-2, 2] {
                n += 1;
                let (_, foot) = self.mech(g, IVec3::new(x, -1, z), IVec3::NEG_Y, IVec3::Z, BlockKind::Piston, &format!("Leg {n}"));
                self.armor(foot, [IVec3::new(0, -1, 0)]);
            }
        }
        // Rear ramp: hinge in the floor, 3x4 plate; +88 deg closes it upwards.
        let (_, ramp) = self.mech(g, IVec3::new(0, 0, 3), IVec3::Z, IVec3::Y, BlockKind::Hinge, "Ramp Hinge");
        self.armor(ramp, (-1..=1).flat_map(|x| (0..4).map(move |z| IVec3::new(x, 0, z))).filter(|c| *c != IVec3::ZERO));
        let pb = self.grids[&g].blocks[&IVec3::new(1, 1, -2)].id;
        let _ = self.upload_script(pb, &examples::shuttle_autopilot());
        g
    }

    /// A small quad-style drone: four lift thrusters on the corners, one thruster
    /// per other direction on the edges, a gyroscope in the middle, a sensor on top
    /// and a computer running the follow script. Spawn its origin 1.6 m above ground.
    fn spawn_drone(&mut self, pos: DVec3, orientation: DQuat) -> GridId {
        let g = self.create_grid(pos, orientation, false, None);
        let ident = rot(IVec3::NEG_Z, IVec3::Y);
        let b = |w: &mut World, cell: IVec3, r: u8, k: BlockKind, name: &str| {
            w.insert_block(g, cell, r, k, None, Some(name.to_string()), false)
        };
        b(self, IVec3::ZERO, ident, BlockKind::Gyro, "Drone Gyro");
        let pb = b(self, IVec3::Y, ident, BlockKind::Programmable, "Drone Computer");
        b(self, IVec3::new(0, 2, 0), ident, BlockKind::Sensor, "Drone Sensor");
        let mut n = 0;
        for x in [-1, 1] {
            for z in [-1, 1] {
                n += 1;
                b(self, IVec3::new(x, 0, z), rot(IVec3::Y, IVec3::Z), BlockKind::Thruster, &format!("Lift Thruster {n}"));
            }
        }
        // Edge thrusters push through the centre line, so they cause no yaw.
        b(self, IVec3::new(0, 0, 1), rot(IVec3::NEG_Z, IVec3::Y), BlockKind::Thruster, "Forward Thruster");
        b(self, IVec3::new(0, 0, -1), rot(IVec3::Z, IVec3::Y), BlockKind::Thruster, "Back Thruster");
        b(self, IVec3::new(1, 0, 0), rot(IVec3::NEG_X, IVec3::Y), BlockKind::Thruster, "Left Thruster");
        b(self, IVec3::new(-1, 0, 0), rot(IVec3::X, IVec3::Y), BlockKind::Thruster, "Right Thruster");
        b(self, IVec3::NEG_Y, rot(IVec3::NEG_Y, IVec3::Z), BlockKind::Thruster, "Down Thruster");
        let _ = self.upload_script(pb, &examples::drone_follow());
        g
    }

    /// A 17 x 9 x 61 m walk-in frigate (about 4,200 blocks): a hollow armour hull
    /// with a tapered nose, two decks joined by a hatch, a bridge cockpit in
    /// the nose, airlocks on both sides, a rear cargo door, two sensor-guided
    /// turrets on the roof and hundreds of thrusters on all six sides.
    ///
    /// Grid frame: -Z forward, +Y up, the origin is the floor centre. The lift
    /// thrusters hang 1.5 m below the floor, so spawn it 1.6 m above ground.
    fn spawn_frigate(&mut self, pos: DVec3, orientation: DQuat) -> GridId {
        // Hull volume: main body z -20..30, nose tapering to 5 x 4 m at z = -30.
        fn half_width(z: i32) -> i32 {
            if z >= -20 { 8 } else { 2 + (z + 30) / 3 }
        }
        fn height(z: i32) -> i32 {
            if z >= -20 { 8 } else { 3 + (z + 30) / 2 }
        }
        fn inside(c: IVec3) -> bool {
            (-30..=30).contains(&c.z) && c.y >= 0 && c.y <= height(c.z) && c.x.abs() <= half_width(c.z)
        }
        const DIRS: [IVec3; 6] = [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z];
        let g = self.create_grid(pos, orientation, false, None);
        let ident = rot(IVec3::NEG_Z, IVec3::Y);

        // Special cells in the shell.
        let cockpit = IVec3::new(0, 2, -30);
        let mut doors: Vec<(IVec3, u8, String)> = Vec::new();
        for (side, x) in [("Port", -8), ("Starboard", 8)] {
            for (lvl, y) in [("Lower", 1), ("Upper", 2)] {
                for z in [-2, 14] {
                    let n = if z < 0 { "Fore" } else { "Aft" };
                    doors.push((IVec3::new(x, y, z), rot(IVec3::new(x.signum(), 0, 0), IVec3::Y), format!("{side} {n} Airlock {lvl}")));
                }
            }
        }
        let mut n = 0;
        for y in 1..=3 {
            for x in -2..=2 {
                n += 1;
                doors.push((IVec3::new(x, y, 30), rot(IVec3::Z, IVec3::Y), format!("Cargo Door {n}")));
            }
        }
        let special: HashSet<IVec3> = doors.iter().map(|d| d.0).chain([cockpit]).collect();

        // Shell, plus the deck between the two levels (with a hatch).
        let mut armor = Vec::new();
        for z in -30..=30 {
            for y in 0..=height(z) {
                for x in -half_width(z)..=half_width(z) {
                    let c = IVec3::new(x, y, z);
                    let shell = DIRS.iter().any(|d| !inside(c + *d));
                    let deck = y == 4 && z >= -20 && z < 30 && !((-1..=0).contains(&x) && (8..=9).contains(&z));
                    if (shell || deck) && !special.contains(&c) {
                        armor.push(c);
                    }
                }
            }
        }
        // Interior bulkheads on the lower deck with a 3-wide passage.
        for z in [-12, 4, 20] {
            for x in -7i32..=7 {
                for y in 1..=3 {
                    if x.abs() > 1 {
                        armor.push(IVec3::new(x, y, z));
                    }
                }
            }
        }
        self.armor(g, armor);
        let b = |w: &mut World, cell: IVec3, r: u8, k: BlockKind, name: &str| {
            w.insert_block(g, cell, r, k, None, Some(name.to_string()), false)
        };
        b(self, cockpit, ident, BlockKind::Cockpit, "Frigate Bridge");
        for (cell, r, name) in &doors {
            b(self, *cell, *r, BlockKind::Door, name);
        }
        let pb = b(self, IVec3::new(-2, 1, -24), ident, BlockKind::Programmable, "Frigate Computer");
        for (i, c) in [(-6, -16), (6, -16), (-6, 26), (6, 26)].into_iter().enumerate() {
            b(self, IVec3::new(c.0, 1, c.1), ident, BlockKind::Gyro, &format!("Frigate Gyro {}", i + 1));
        }
        b(self, IVec3::new(0, 9, -16), ident, BlockKind::Sensor, "Frigate Sensor");

        // Thrusters: a thruster pushing along `f` sits outside the hull with
        // the hull at `c + f` and free space behind its nozzle at `c - f`.
        let reserved = |c: IVec3| {
            // Keep the cockpit view, the airlocks, the cargo door and the turrets clear.
            (c.z <= -31 && c.x.abs() <= 1 && (1..=3).contains(&c.y))
                || (c.x.abs() >= 9 && (c.z == -2 || c.z == 14) && c.y <= 3)
                || (c.z >= 31 && c.x.abs() <= 3 && c.y <= 4)
                || (c.y >= 9 && c.x.abs() <= 2 && ((-10..=-6).contains(&c.z) || (18..=22).contains(&c.z) || c.z == -16))
        };
        let pattern = |f: IVec3, c: IVec3| match (f.x, f.y, f.z) {
            (0, 1, 0) => c.x.rem_euclid(2) == 0 && c.z.rem_euclid(2) == 0, // lift: every other cell under the belly
            (0, -1, 0) => c.x.rem_euclid(4) == 0 && c.z.rem_euclid(4) == 0,
            (0, 0, -1) => c.x.rem_euclid(2) == 0 && c.y.rem_euclid(2) == 0, // main engines on the stern
            (0, 0, 1) => (c.x + c.y).rem_euclid(2) == 0,                    // retro thrusters on the nose and shoulders
            _ => c.y.rem_euclid(3) == 1 && c.z.rem_euclid(3) == 0,          // lateral
        };
        let mut placed: HashSet<IVec3> = HashSet::new();
        let mut counts: HashMap<IVec3, usize> = HashMap::new();
        for f in DIRS {
            let mut cells = Vec::new();
            for z in -32..=32 {
                for y in -1..=10 {
                    for x in -10..=10 {
                        let c = IVec3::new(x, y, z);
                        if !inside(c) && inside(c + f) && !inside(c - f) && !reserved(c) && pattern(f, c) && !placed.contains(&c) {
                            cells.push(c);
                        }
                    }
                }
            }
            let name = match (f.x, f.y, f.z) {
                (0, 1, 0) => "Lift",
                (0, -1, 0) => "Down",
                (0, 0, -1) => "Main",
                (0, 0, 1) => "Retro",
                (1, 0, 0) => "Starboard",
                _ => "Port",
            };
            let up = if f.y == 0 { IVec3::Y } else { IVec3::Z };
            for (i, c) in cells.into_iter().enumerate() {
                placed.insert(c);
                *counts.entry(f).or_default() += 1;
                b(self, c, rot(f, up), BlockKind::Thruster, &format!("{name} Thruster {}", i + 1));
            }
        }

        // Two turrets: rotor (yaw) -> housing -> hinge (pitch) -> barrel.
        for (i, z) in [(1, -8), (2, 20)] {
            let (_, head) = self.mech(g, IVec3::new(0, 9, z), IVec3::Y, IVec3::Z, BlockKind::Rotor, &format!("Turret {i} Rotor"));
            self.armor(head, [IVec3::new(1, 0, 0), IVec3::new(-1, 0, 0), IVec3::new(1, 1, 0), IVec3::new(-1, 1, 0), IVec3::new(0, 0, 1), IVec3::new(0, 0, -1)]);
            let (_, barrel) = self.mech(head, IVec3::Y, IVec3::Y, IVec3::Z, BlockKind::Hinge, &format!("Turret {i} Hinge"));
            self.armor(barrel, (1..=4).map(|y| IVec3::new(0, y, 0)));
        }
        let _ = self.upload_script(pb, &examples::FRIGATE);
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
