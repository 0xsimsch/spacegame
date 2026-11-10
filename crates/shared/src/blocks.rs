//! Static block definitions.

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
}

/// Blocks the player can pick in build mode, in hot-bar order.
pub const BUILDABLE: [BlockKind; 6] = [
    BlockKind::Armor,
    BlockKind::Thruster,
    BlockKind::Piston,
    BlockKind::Hinge,
    BlockKind::Programmable,
    BlockKind::Cockpit,
];

/// Maximum thrust of one thruster in newtons.
pub const THRUSTER_MAX_FORCE: f64 = 30_000.0;
/// Piston travel range in metres (extension beyond the rest position).
pub const PISTON_MIN: f64 = 0.0;
pub const PISTON_MAX: f64 = 5.0;
pub const PISTON_MAX_SPEED: f64 = 2.0;
pub const PISTON_MAX_FORCE: f64 = 200_000.0;
/// Hinge angle limits in degrees.
pub const HINGE_LIMIT_DEG: f64 = 90.0;
pub const HINGE_MAX_SPEED_DEG: f64 = 90.0;
pub const HINGE_MAX_TORQUE: f64 = 1_500_000.0;

impl BlockKind {
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
        }
    }

    /// Whether the block shows up in the control panel (has something to configure).
    pub fn is_terminal(self) -> bool {
        !matches!(self, BlockKind::Armor | BlockKind::PistonTop | BlockKind::HingeTop)
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
        }
    }
}
