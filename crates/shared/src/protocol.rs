//! Network messages. Encoded with postcard.
//!
//! Channels (renet `DefaultChannel`): `Unreliable` for `Input`/`Snapshot`,
//! `ReliableOrdered` for everything else.

use crate::blocks::BlockKind;
use crate::{BlockId, GridId, PlayerId};
use glam::{DQuat, DVec3, IVec3, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PlayerInput {
    /// Increases by one each client frame; echoed in snapshots for prediction.
    pub seq: u32,
    /// Local movement (x right, y up, z back), each component in -1..=1.
    pub movement: Vec3,
    /// Rotation input while piloting (pitch, yaw, roll), each in -1..=1.
    pub rotation: Vec3,
    /// World-space look orientation of the player (client authoritative).
    pub look: DQuat,
    pub dampeners: bool,
    pub jetpack: bool,
    pub cruise: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BlockProp {
    ThrusterOverride(f32),
    PistonVelocity(f32),
    /// Hinge or rotor speed in deg/s.
    HingeVelocity(f32),
    /// Hinge or rotor target angle; `None` switches back to velocity mode.
    HingeTarget(Option<f32>),
    ScriptFrequency(u32),
    /// Pistons/hinges/rotors: switch the motor off so the joint moves freely
    /// under external forces (limits still apply).
    Free(bool),
    /// Gyroscope: target angular velocity in deg/s (grid frame); `None` = off.
    GyroOverride(Option<[f32; 3]>),
    DoorOpen(bool),
    /// Sensor detection range in metres.
    SensorRange(f32),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClientMsg {
    Hello { name: String },
    Input(PlayerInput),
    PlaceBlock { grid: GridId, cell: IVec3, rot: u8, kind: BlockKind },
    RemoveBlock { grid: GridId, cell: IVec3 },
    NewGrid { pos: DVec3, rot: DQuat, kind: BlockKind, is_static: bool },
    RenameBlock { block: BlockId, name: String },
    SetBlockProp { block: BlockId, prop: BlockProp },
    /// Ask the server for the current script/output of a programmable block.
    RequestScript { block: BlockId },
    UploadScript { block: BlockId, source: String },
    RunScript { block: BlockId, arg: String },
    EnterSeat { block: BlockId },
    ExitSeat,
    /// Chat or `/command`.
    Chat(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockData {
    pub id: BlockId,
    pub kind: BlockKind,
    pub cell: IVec3,
    pub rot: u8,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GridData {
    pub id: GridId,
    pub is_static: bool,
    pub pos: DVec3,
    pub rot: DQuat,
    pub blocks: Vec<BlockData>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayerState {
    pub id: PlayerId,
    pub pos: DVec3,
    pub vel: DVec3,
    pub look: DQuat,
    pub seat: Option<BlockId>,
    pub grounded: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GridState {
    pub id: GridId,
    pub pos: DVec3,
    pub rot: DQuat,
    pub linvel: Vec3,
    pub angvel: Vec3,
}

/// A per-block animated value: thruster output (0..1), piston extension (m),
/// hinge angle (deg), door open (1/0) or number of players a sensor detects.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockValue {
    pub id: BlockId,
    pub value: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub tick: u64,
    /// Last input sequence the server applied for this client.
    pub ack_input: u32,
    pub players: Vec<PlayerState>,
    pub grids: Vec<GridState>,
    pub values: Vec<BlockValue>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ServerStats {
    pub tick_ms: f32,
    pub script_ms: f32,
    pub physics_ms: f32,
    pub grids: u32,
    pub blocks: u32,
    pub scripts: u32,
    pub players: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScriptInfo {
    pub block: BlockId,
    pub source: String,
    pub output: Vec<String>,
    pub error: Option<String>,
    pub frequency: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockControlInfo {
    pub block: BlockId,
    pub thruster_override: f32,
    pub piston_velocity: f32,
    pub hinge_velocity: f32,
    pub hinge_target: Option<f32>,
    pub free: bool,
    pub gyro_override: Option<[f32; 3]>,
    pub door_open: bool,
    pub sensor_range: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ServerMsg {
    Welcome { player: PlayerId, tick: u64 },
    PlayerInfo { id: PlayerId, name: String },
    PlayerLeft { id: PlayerId },
    FullGrid(GridData),
    GridRemoved(GridId),
    BlockAdded { grid: GridId, block: BlockData },
    BlockRemoved { grid: GridId, block: BlockId },
    BlockRenamed { block: BlockId, name: String },
    BlockControl(BlockControlInfo),
    Snapshot(Snapshot),
    ScriptInfo(ScriptInfo),
    /// New output lines of a programmable block (sent to everyone who knows the grid).
    ScriptOutput { block: BlockId, lines: Vec<String>, error: Option<String> },
    Chat { from: String, text: String },
    Stats(ServerStats),
}
