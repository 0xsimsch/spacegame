//! Static block definitions.

use crate::orient;
use glam::{DVec3, IVec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlockKind {
    Armor,
    Thruster,
    Piston,
    /// Created automatically on top of a piston (its own sub-grid).
    PistonTop,
    Hinge,
    /// Created automatically on top of a hinge (its own sub-grid).
    HingeTop,
    Programmable,
    Cockpit,
    /// Spins without limit around its forward axis.
    Rotor,
    /// Created automatically on top of a rotor (its own sub-grid).
    RotorTop,
    /// Steers the grid's angular velocity when given an override (scripts/panel).
    Gyro,
    /// Solid when closed; when open its collider is off and you can walk through.
    Door,
    /// Detects players within a configurable range (read by scripts).
    Sensor,
    /// A 2x2x2 thruster: the anchor cell is its front-left-bottom corner,
    /// it extends to the right, up and backwards (toward the nozzle).
    LargeThruster,
    /// Magnetic pad on its forward face: locks onto a grid or the ground in front of it.
    LandingGear,
    /// Two merge blocks facing each other pull their grids together and fuse them
    /// into one; disabling one splits them apart again.
    MergeBlock,
    /// Docking port: locks to another connector facing it, the grids stay separate.
    Connector,
}

/// Blocks the player can pick in build mode, in hot-bar order.
pub const BUILDABLE: [BlockKind; 14] = [
    BlockKind::Armor,
    BlockKind::Thruster,
    BlockKind::Piston,
    BlockKind::Hinge,
    BlockKind::Rotor,
    BlockKind::Programmable,
    BlockKind::Cockpit,
    BlockKind::Gyro,
    BlockKind::Door,
    BlockKind::Sensor,
    BlockKind::LargeThruster,
    BlockKind::LandingGear,
    BlockKind::MergeBlock,
    BlockKind::Connector,
];

/// Maximum thrust of one thruster in newtons.
pub const THRUSTER_MAX_FORCE: f64 = 30_000.0;
/// The 2x2x2 thruster: 8 cells of volume, but almost 7x the force of a small one.
pub const LARGE_THRUSTER_MAX_FORCE: f64 = 200_000.0;
/// Piston travel range in metres (extension beyond the rest position).
pub const PISTON_MIN: f64 = 0.0;
pub const PISTON_MAX: f64 = 5.0;
pub const PISTON_MAX_SPEED: f64 = 2.0;
pub const PISTON_MAX_FORCE: f64 = 200_000.0;
/// Hinge angle limits in degrees.
pub const HINGE_LIMIT_DEG: f64 = 90.0;
pub const HINGE_MAX_SPEED_DEG: f64 = 90.0;
pub const HINGE_MAX_TORQUE: f64 = 1_500_000.0;
/// Rotor speed limit in degrees per second (no angle limits).
pub const ROTOR_MAX_SPEED_DEG: f64 = 180.0;
pub const ROTOR_MAX_TORQUE: f64 = 1_500_000.0;
/// Gyroscope override limit (deg/s per axis).
pub const GYRO_MAX_RATE_DEG: f64 = 90.0;
/// Sensor detection range in metres (default and maximum).
pub const SENSOR_DEFAULT_RANGE: f64 = 50.0;
pub const SENSOR_MAX_RANGE: f64 = 500.0;

impl BlockKind {
    /// Pistons, hinges and rotors: blocks that carry a head sub-grid on a joint.
    pub fn is_mech(self) -> bool {
        matches!(self, BlockKind::Piston | BlockKind::Hinge | BlockKind::Rotor)
    }

    pub fn is_thruster(self) -> bool {
        matches!(self, BlockKind::Thruster | BlockKind::LargeThruster)
    }

    /// Maximum thrust in newtons (0 for non-thrusters).
    pub fn thrust(self) -> f64 {
        match self {
            BlockKind::Thruster => THRUSTER_MAX_FORCE,
            BlockKind::LargeThruster => LARGE_THRUSTER_MAX_FORCE,
            _ => 0.0,
        }
    }

    /// Edge length in cells (blocks are cubes of 1 or 2 cells).
    pub fn size(self) -> i32 {
        if self == BlockKind::LargeThruster { 2 } else { 1 }
    }

    /// Cells covered by a block at `cell` with orientation `rot` (the anchor first).
    pub fn footprint(self, cell: IVec3, rot: u8) -> Vec<IVec3> {
        let n = self.size();
        let (r, u, b) = (orient::right(rot), orient::up(rot), -orient::forward(rot));
        let mut out = Vec::with_capacity((n * n * n) as usize);
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    out.push(cell + r * x + u * y + b * z);
                }
            }
        }
        out
    }

    /// Centre of the block relative to its anchor cell, in grid coordinates.
    pub fn center_offset(self, rot: u8) -> DVec3 {
        let h = (self.size() - 1) as f64 * 0.5;
        (orient::right(rot).as_dvec3() + orient::up(rot).as_dvec3() - orient::forward(rot).as_dvec3()) * h
    }

    /// Joints measured in degrees (hinges and rotors).
    pub fn is_angular(self) -> bool {
        matches!(self, BlockKind::Hinge | BlockKind::Rotor)
    }

    pub fn mass(self) -> f64 {
        match self {
            BlockKind::Armor => 100.0,
            BlockKind::Thruster => 150.0,
            BlockKind::Piston => 200.0,
            BlockKind::PistonTop => 150.0,
            BlockKind::Hinge => 200.0,
            BlockKind::HingeTop => 150.0,
            BlockKind::Programmable => 150.0,
            BlockKind::Cockpit => 150.0,
            BlockKind::Rotor => 250.0,
            BlockKind::RotorTop => 150.0,
            BlockKind::Gyro => 300.0,
            BlockKind::Door => 80.0,
            BlockKind::Sensor => 120.0,
            BlockKind::LargeThruster => 900.0,
            BlockKind::LandingGear => 150.0,
            BlockKind::MergeBlock => 150.0,
            BlockKind::Connector => 150.0,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            BlockKind::Armor => "Armor Block",
            BlockKind::Thruster => "Thruster",
            BlockKind::Piston => "Piston",
            BlockKind::PistonTop => "Piston Head",
            BlockKind::Hinge => "Hinge",
            BlockKind::HingeTop => "Hinge Head",
            BlockKind::Programmable => "Programmable Block",
            BlockKind::Cockpit => "Cockpit",
            BlockKind::Rotor => "Rotor",
            BlockKind::RotorTop => "Rotor Head",
            BlockKind::Gyro => "Gyroscope",
            BlockKind::Door => "Door",
            BlockKind::Sensor => "Sensor",
            BlockKind::LargeThruster => "Large Thruster",
            BlockKind::LandingGear => "Landing Gear",
            BlockKind::MergeBlock => "Merge Block",
            BlockKind::Connector => "Connector",
        }
    }

    /// Lowercase identifier used by the scripting API (`grid:blocks("thruster")`).
    pub fn api_name(self) -> &'static str {
        match self {
            BlockKind::Armor => "armor",
            BlockKind::Thruster => "thruster",
            BlockKind::Piston => "piston",
            BlockKind::PistonTop => "piston_top",
            BlockKind::Hinge => "hinge",
            BlockKind::HingeTop => "hinge_top",
            BlockKind::Programmable => "programmable",
            BlockKind::Cockpit => "cockpit",
            BlockKind::Rotor => "rotor",
            BlockKind::RotorTop => "rotor_top",
            BlockKind::Gyro => "gyro",
            BlockKind::Door => "door",
            BlockKind::Sensor => "sensor",
            // Scripts treat both sizes as thrusters (`.max_thrust` tells them apart).
            BlockKind::LargeThruster => "thruster",
            BlockKind::LandingGear => "landing_gear",
            BlockKind::MergeBlock => "merge",
            BlockKind::Connector => "connector",
        }
    }

    /// Whether the block shows up in the control panel (has something to configure).
    pub fn is_terminal(self) -> bool {
        !matches!(self, BlockKind::Armor | BlockKind::PistonTop | BlockKind::HingeTop | BlockKind::RotorTop)
    }

    pub fn buildable(self) -> bool {
        BUILDABLE.contains(&self)
    }

    /// Display colour (linear RGB) used by the client.
    pub fn color(self) -> [f32; 3] {
        match self {
            BlockKind::Armor => [0.55, 0.57, 0.6],
            BlockKind::Thruster => [0.35, 0.36, 0.4],
            BlockKind::Piston | BlockKind::PistonTop => [0.85, 0.6, 0.15],
            BlockKind::Hinge | BlockKind::HingeTop => [0.2, 0.55, 0.85],
            BlockKind::Programmable => [0.2, 0.7, 0.35],
            BlockKind::Cockpit => [0.75, 0.75, 0.8],
            BlockKind::Rotor | BlockKind::RotorTop => [0.65, 0.3, 0.75],
            BlockKind::Gyro => [0.85, 0.75, 0.2],
            BlockKind::Door => [0.45, 0.5, 0.55],
            BlockKind::Sensor => [0.15, 0.7, 0.75],
            BlockKind::LargeThruster => [0.3, 0.31, 0.36],
            BlockKind::LandingGear => [0.8, 0.6, 0.1],
            BlockKind::MergeBlock => [0.75, 0.75, 0.3],
            BlockKind::Connector => [0.35, 0.55, 0.6],
        }
    }
}
