//! Types shared by server, simulation and client: constants, block definitions,
//! cube orientations, solar-system data and the network protocol.

pub mod blocks;
pub mod examples;
pub mod orient;
pub mod protocol;
pub mod solar;

pub use glam::{DQuat, DVec3, IVec3, Quat, Vec3};

/// Simulation ticks per second.
pub const TICK_HZ: u32 = 60;
pub const TICK_DT: f64 = 1.0 / TICK_HZ as f64;
/// Edge length of one grid cell in metres.
pub const CELL_SIZE: f64 = 1.0;
/// Snapshots are sent every N ticks.
pub const SNAPSHOT_EVERY: u64 = 2;
/// Entities farther than this from a client are not replicated to it.
pub const INTEREST_RADIUS: f64 = 20_000.0;
/// Normal speed cap (m/s) and the cap while cruise mode is on.
pub const SPEED_CAP: f64 = 150.0;
pub const CRUISE_SPEED_CAP: f64 = 5_000.0;
/// Bump when the protocol changes so old clients cannot connect.
pub const PROTOCOL_ID: u64 = 0x5350_4143_0000_0004;
pub const DEFAULT_PORT: u16 = 47_800;

pub type GridId = u32;
pub type BlockId = u32;
pub type PlayerId = u32;

/// Encode any protocol message.
pub fn encode<T: serde::Serialize>(msg: &T) -> Vec<u8> {
    postcard::to_allocvec(msg).expect("serialisation cannot fail")
}

/// Decode any protocol message; `None` if malformed.
pub fn decode<'a, T: serde::Deserialize<'a>>(bytes: &'a [u8]) -> Option<T> {
    postcard::from_bytes(bytes).ok()
}
