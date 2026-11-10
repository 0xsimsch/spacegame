//! Sandboxed player scripts for programmable blocks (Luau via mlua).
//!
//! Execution model: every run receives an immutable [`GridView`] snapshot and
//! produces a list of [`Command`]s. Scripts never touch the world directly, so
//! many hosts can run in parallel and the server applies all commands at once.
//!
//! The rest of the engine only talks to the [`ScriptHost`] type, so another
//! backend (e.g. WebAssembly) can be added later behind the same API.

use glam::DVec3;
use mlua::{Function, Lua, MetaMethod, UserData, UserDataMethods, Value, VmState};
use shared::BlockId;
use shared::blocks::BlockKind;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Interrupt callbacks allowed per run (Luau calls it on loop back-edges and calls).
pub const INTERRUPT_BUDGET: u64 = 50_000;
/// Wall-clock limit per run.
pub const TIME_BUDGET: Duration = Duration::from_millis(5);
pub const MEMORY_LIMIT: usize = 4 * 1024 * 1024;
pub const MAX_OUTPUT_LINES: usize = 50;
pub const MAX_STORAGE_BYTES: usize = 64 * 1024;

/// Everything a script can read about one block. Vectors are expressed in the
/// frame of the programmable block's own grid.
#[derive(Clone, Debug)]
pub struct BlockInfo {
    pub id: BlockId,
    pub kind: BlockKind,
    pub name: String,
    pub forward: DVec3,
    pub up: DVec3,
    pub position: DVec3,
    pub same_grid: bool,
    pub thrust_max: f64,
    pub thrust_override: f64,
    pub thrust_current: f64,
    pub piston_position: f64,
    pub piston_velocity: f64,
    pub hinge_angle: f64,
    pub hinge_velocity: f64,
    pub hinge_target: Option<f64>,
    pub free: bool,
    pub gyro_override: Option<DVec3>,
    pub door_open: bool,
}

#[derive(Clone, Debug, Default)]
pub struct GridInfo {
    pub position: DVec3,
    /// World-space velocity.
    pub velocity: DVec3,
    /// Velocity in the grid frame.
    pub local_velocity: DVec3,
    /// Angular velocity in the grid frame (rad/s).
    pub angular_velocity: DVec3,
    /// Gravity acceleration in the grid frame.
    pub gravity: DVec3,
    pub altitude: f64,
    pub planet: String,
    pub mass: f64,
    pub speed: f64,
    pub cruise: bool,
}

#[derive(Clone, Debug, Default)]
pub struct GridView {
    pub me: BlockId,
    pub me_name: String,
    pub tick: u64,
    pub dt: f64,
    pub grid: GridInfo,
    pub blocks: Vec<BlockInfo>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    ThrusterOverride(BlockId, f64),
    PistonVelocity(BlockId, f64),
    HingeVelocity(BlockId, f64),
    HingeTarget(BlockId, Option<f64>),
    SetFrequency(u32),
    SetCruise(bool),
    SetFree(BlockId, bool),
    GyroOverride(BlockId, Option<DVec3>),
    DoorOpen(BlockId, bool),
}

#[derive(Debug, Default)]
pub struct RunResult {
    pub commands: Vec<Command>,
    pub output: Vec<String>,
    pub error: Option<String>,
}

/// Per-run scratch data stored in the Lua state's app data.
#[derive(Default)]
struct RunCtx {
    view: GridView,
    commands: Vec<Command>,
    output: Vec<String>,
}

#[derive(Clone, Copy)]
struct BlockRef(BlockId);

fn with_block<R>(lua: &Lua, id: BlockId, f: impl FnOnce(&BlockInfo) -> R) -> mlua::Result<R> {
    let ctx = lua.app_data_ref::<RunCtx>().ok_or_else(|| mlua::Error::runtime("no context"))?;
    let b = ctx
        .view
        .blocks
        .iter()
        .find(|b| b.id == id)
        .ok_or_else(|| mlua::Error::runtime("block no longer exists"))?;
    Ok(f(b))
}

fn push(lua: &Lua, c: Command) -> mlua::Result<()> {
    let mut ctx = lua.app_data_mut::<RunCtx>().ok_or_else(|| mlua::Error::runtime("no context"))?;
    ctx.commands.push(c);
    Ok(())
}

fn vec_value(v: DVec3) -> Value {
    Value::Vector(mlua::Vector::new(v.x as f32, v.y as f32, v.z as f32))
}

fn kind_check(lua: &Lua, id: BlockId, want: BlockKind, what: &str) -> mlua::Result<()> {
    let k = with_block(lua, id, |b| b.kind)?;
    if k != want {
        return Err(mlua::Error::runtime(format!("{what} called on a {} block", k.api_name())));
    }
    Ok(())
}

impl UserData for BlockRef {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("set_override", |lua, this, v: Value| {
            match (with_block(lua, this.0, |b| b.kind)?, v) {
                (BlockKind::Thruster, Value::Number(n)) => push(lua, Command::ThrusterOverride(this.0, n.clamp(0.0, 1.0))),
                (BlockKind::Thruster, Value::Integer(n)) => push(lua, Command::ThrusterOverride(this.0, (n as f64).clamp(0.0, 1.0))),
                (BlockKind::Gyro, Value::Vector(w)) => {
                    push(lua, Command::GyroOverride(this.0, Some(DVec3::new(w.x() as f64, w.y() as f64, w.z() as f64))))
                }
                (BlockKind::Gyro, Value::Nil) => push(lua, Command::GyroOverride(this.0, None)),
                (BlockKind::Thruster, _) => Err(mlua::Error::runtime("thruster:set_override expects a number 0..1")),
                (BlockKind::Gyro, _) => Err(mlua::Error::runtime("gyro:set_override expects a vector (deg/s) or nil")),
                (k, _) => Err(mlua::Error::runtime(format!("set_override called on a {} block", k.api_name()))),
            }
        });
        m.add_method("set_velocity", |lua, this, v: f64| {
            let kind = with_block(lua, this.0, |b| b.kind)?;
            match kind {
                BlockKind::Piston => push(lua, Command::PistonVelocity(this.0, v)),
                BlockKind::Hinge | BlockKind::Rotor => push(lua, Command::HingeVelocity(this.0, v)),
                k => Err(mlua::Error::runtime(format!("set_velocity called on a {} block", k.api_name()))),
            }
        });
        m.add_method("set_free", |lua, this, on: bool| {
            let k = with_block(lua, this.0, |b| b.kind)?;
            if !k.is_mech() {
                return Err(mlua::Error::runtime(format!("set_free called on a {} block", k.api_name())));
            }
            push(lua, Command::SetFree(this.0, on))
        });
        m.add_method("set_open", |lua, this, on: bool| {
            kind_check(lua, this.0, BlockKind::Door, "set_open")?;
            push(lua, Command::DoorOpen(this.0, on))
        });
        m.add_method("open", |lua, this, ()| {
            kind_check(lua, this.0, BlockKind::Door, "open")?;
            push(lua, Command::DoorOpen(this.0, true))
        });
        m.add_method("close", |lua, this, ()| {
            kind_check(lua, this.0, BlockKind::Door, "close")?;
            push(lua, Command::DoorOpen(this.0, false))
        });
        m.add_method("toggle", |lua, this, ()| {
            kind_check(lua, this.0, BlockKind::Door, "toggle")?;
            let open = with_block(lua, this.0, |b| b.door_open)?;
            push(lua, Command::DoorOpen(this.0, !open))
        });
        m.add_method("set_target_angle", |lua, this, v: Option<f64>| {
            let k = with_block(lua, this.0, |b| b.kind)?;
            if !k.is_angular() {
                return Err(mlua::Error::runtime(format!("set_target_angle called on a {} block", k.api_name())));
            }
            push(lua, Command::HingeTarget(this.0, v))
        });
        m.add_meta_method(MetaMethod::ToString, |lua, this, ()| {
            with_block(lua, this.0, |b| format!("{}<{}>", b.kind.api_name(), b.name))
        });
        m.add_meta_method(MetaMethod::Eq, |_, this, other: mlua::UserDataRef<BlockRef>| Ok(this.0 == other.0));
        m.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            with_block(lua, this.0, |b| -> mlua::Result<Value> {
                Ok(match key.as_str() {
                    "id" => Value::Integer(b.id as _),
                    "name" => Value::String(lua.create_string(&b.name)?),
                    "type" => Value::String(lua.create_string(b.kind.api_name())?),
                    "forward" => vec_value(b.forward),
                    "up" => vec_value(b.up),
                    "position" => vec_value(b.position),
                    "same_grid" => Value::Boolean(b.same_grid),
                    "max_thrust" => Value::Number(b.thrust_max),
                    "override" => match b.kind {
                        BlockKind::Gyro => b.gyro_override.map(vec_value).unwrap_or(Value::Nil),
                        _ => Value::Number(b.thrust_override),
                    },
                    "is_open" => Value::Boolean(b.door_open),
                    "current_thrust" => Value::Number(b.thrust_current),
                    "position_m" | "extension" => Value::Number(b.piston_position),
                    "min" => Value::Number(if b.kind == BlockKind::Rotor {
                        -180.0
                    } else if b.kind == BlockKind::Hinge {
                        -shared::blocks::HINGE_LIMIT_DEG
                    } else {
                        shared::blocks::PISTON_MIN
                    }),
                    "max" => Value::Number(if b.kind == BlockKind::Rotor {
                        180.0
                    } else if b.kind == BlockKind::Hinge {
                        shared::blocks::HINGE_LIMIT_DEG
                    } else {
                        shared::blocks::PISTON_MAX
                    }),
                    "velocity" => Value::Number(match b.kind {
                        BlockKind::Hinge | BlockKind::Rotor => b.hinge_velocity,
                        _ => b.piston_velocity,
                    }),
                    "angle" => Value::Number(b.hinge_angle),
                    "free" => Value::Boolean(b.free),
                    "target_angle" => b.hinge_target.map(Value::Number).unwrap_or(Value::Nil),
                    _ => Value::Nil,
                })
            })?
        });
    }
}

struct GridRef;

impl UserData for GridRef {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("block", |lua, _, name: String| {
            let ctx = lua.app_data_ref::<RunCtx>().unwrap();
            Ok(ctx.view.blocks.iter().find(|b| b.name == name).map(|b| BlockRef(b.id)))
        });
        m.add_method("blocks", |lua, _, kind: Option<String>| {
            let ids: Vec<BlockRef> = {
                let ctx = lua.app_data_ref::<RunCtx>().unwrap();
                ctx.view
                    .blocks
                    .iter()
                    .filter(|b| kind.as_deref().is_none_or(|k| b.kind.api_name() == k))
                    .map(|b| BlockRef(b.id))
                    .collect()
            };
            lua.create_sequence_from(ids)
        });
        m.add_method("set_cruise", |lua, _, on: bool| push(lua, Command::SetCruise(on)));
        m.add_meta_method(MetaMethod::Index, |lua, _, key: String| {
            let ctx = lua.app_data_ref::<RunCtx>().unwrap();
            let g = &ctx.view.grid;
            Ok(match key.as_str() {
                "velocity" => vec_value(g.local_velocity),
                "world_velocity" => vec_value(g.velocity),
                "angular_velocity" => vec_value(g.angular_velocity),
                "gravity" => vec_value(g.gravity),
                "altitude" => Value::Number(g.altitude),
                "planet" => Value::String(lua.create_string(&g.planet)?),
                "mass" => Value::Number(g.mass),
                "speed" => Value::Number(g.speed),
                "cruise" => Value::Boolean(g.cruise),
                "position" => {
                    let t = lua.create_table()?;
                    t.set("x", g.position.x)?;
                    t.set("y", g.position.y)?;
                    t.set("z", g.position.z)?;
                    Value::Table(t)
                }
                _ => Value::Nil,
            })
        });
    }
}

struct MeRef;

impl UserData for MeRef {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("set_frequency", |lua, _, n: u32| {
            if ![1, 10, 100].contains(&n) {
                return Err(mlua::Error::runtime("frequency must be 1, 10 or 100"));
            }
            push(lua, Command::SetFrequency(n))
        });
        m.add_meta_method(MetaMethod::Index, |lua, _, key: String| {
            let ctx = lua.app_data_ref::<RunCtx>().unwrap();
            Ok(match key.as_str() {
                "name" => Value::String(lua.create_string(&ctx.view.me_name)?),
                "id" => Value::Integer(ctx.view.me as _),
                "tick" => Value::Integer(ctx.view.tick as _),
                "dt" => Value::Number(ctx.view.dt),
                _ => Value::Nil,
            })
        });
    }
}

/// A compiled script plus its Lua state.
pub struct ScriptHost {
    lua: Lua,
    budget: Arc<AtomicU64>,
    deadline: Arc<std::sync::Mutex<Instant>>,
}

fn fmt_value(v: &Value) -> String {
    match v {
        Value::Nil => "nil".into(),
        Value::Boolean(b) => b.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => {
            if n.fract() == 0.0 && n.abs() < 1e15 {
                format!("{n:.0}")
            } else {
                format!("{n:.3}")
            }
        }
        Value::Vector(v) => format!("({:.3}, {:.3}, {:.3})", v.x(), v.y(), v.z()),
        Value::String(s) => s.to_string_lossy(),
        other => other.to_string().unwrap_or_else(|_| other.type_name().to_string()),
    }
}

fn clean_error(e: &mlua::Error) -> String {
    // Keep just the first line of a runtime error, without the Rust traceback.
    let s = match e {
        mlua::Error::CallbackError { cause, .. } => return clean_error(cause),
        other => other.to_string(),
    };
    s.lines()
        .next()
        .unwrap_or("error")
        .replace("runtime error: ", "")
        .trim()
        .to_string()
}

impl ScriptHost {
    /// Compile `source` and run its top level once.
    pub fn new(source: &str, storage: &str, view: &GridView) -> Result<(Self, RunResult), String> {
        let lua = Lua::new();
        let budget = Arc::new(AtomicU64::new(0));
        let deadline = Arc::new(std::sync::Mutex::new(Instant::now() + TIME_BUDGET));
        let host = Self { lua, budget, deadline };
        host.setup().map_err(|e| clean_error(&e))?;
        host.lua.globals().set("storage", storage).map_err(|e| clean_error(&e))?;
        let chunk = host.lua.load(source).set_name("=script");
        let res = host.run_with(view.clone(), None, |lua| chunk.exec().map(|_| { let _ = lua; }));
        if let Some(e) = &res.error {
            return Err(e.clone());
        }
        Ok((host, res))
    }

    fn setup(&self) -> mlua::Result<()> {
        let lua = &self.lua;
        lua.set_memory_limit(MEMORY_LIMIT)?;
        let globals = lua.globals();
        globals.set(
            "print",
            lua.create_function(|lua, args: mlua::MultiValue| {
                let line = args.iter().map(fmt_value).collect::<Vec<_>>().join("\t");
                if let Some(mut ctx) = lua.app_data_mut::<RunCtx>() {
                    ctx.output.push(line);
                }
                Ok(())
            })?,
        )?;
        globals.set("grid", GridRef)?;
        globals.set("me", MeRef)?;
        lua.sandbox(true)?;
        // `grid`/`me` fields change every run: stop Luau from caching `grid.x`
        // lookups as constant imports.
        lua.set_compiler(mlua::chunk::Compiler::new().set_optimization_level(2).set_mutable_globals(["grid", "me"]));
        let budget = self.budget.clone();
        let deadline = self.deadline.clone();
        lua.set_interrupt(move |_| {
            let used = budget.fetch_add(1, Ordering::Relaxed);
            if used > INTERRUPT_BUDGET {
                return Err(mlua::Error::runtime("instruction budget exceeded (infinite loop?)"));
            }
            if used % 256 == 0 && Instant::now() > *deadline.lock().unwrap() {
                return Err(mlua::Error::runtime("time budget exceeded"));
            }
            Ok(VmState::Continue)
        });
        Ok(())
    }

    fn run_with(
        &self,
        view: GridView,
        _arg: Option<&str>,
        f: impl FnOnce(&Lua) -> mlua::Result<()>,
    ) -> RunResult {
        self.budget.store(0, Ordering::Relaxed);
        *self.deadline.lock().unwrap() = Instant::now() + TIME_BUDGET;
        self.lua.set_app_data(RunCtx { view, ..Default::default() });
        let r = f(&self.lua);
        let ctx = self.lua.remove_app_data::<RunCtx>().unwrap_or_default();
        let mut output = ctx.output;
        if output.len() > MAX_OUTPUT_LINES {
            output.drain(..output.len() - MAX_OUTPUT_LINES);
        }
        match r {
            Ok(()) => RunResult { commands: ctx.commands, output, error: None },
            // A failing run applies none of its commands.
            Err(e) => RunResult { commands: Vec::new(), output, error: Some(clean_error(&e)) },
        }
    }

    /// Call the script's `tick(dt)` function (if defined).
    pub fn tick(&self, view: GridView) -> RunResult {
        let dt = view.dt;
        self.run_with(view, None, |lua| match lua.globals().get::<Option<Function>>("tick")? {
            Some(f) => f.call::<()>(dt),
            None => Ok(()),
        })
    }

    /// Call `on_message(arg)` (triggered by the block's Run button).
    pub fn message(&self, view: GridView, arg: &str) -> RunResult {
        self.run_with(view, Some(arg), |lua| match lua.globals().get::<Option<Function>>("on_message")? {
            Some(f) => f.call::<()>(arg.to_string()),
            None => Ok(()),
        })
    }

    /// The script's persistent `storage` string.
    pub fn storage(&self) -> String {
        let s: Option<String> = self.lua.globals().get("storage").ok();
        let mut s = s.unwrap_or_default();
        s.truncate(MAX_STORAGE_BYTES);
        s
    }

    pub fn memory_used(&self) -> usize {
        self.lua.used_memory()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> GridView {
        GridView {
            me: 1,
            me_name: "PB".into(),
            dt: 1.0 / 60.0,
            blocks: vec![
                BlockInfo {
                    id: 2,
                    kind: BlockKind::Piston,
                    name: "Arm".into(),
                    forward: DVec3::Y,
                    up: DVec3::Z,
                    position: DVec3::ZERO,
                    same_grid: true,
                    thrust_max: 0.0,
                    thrust_override: 0.0,
                    thrust_current: 0.0,
                    piston_position: 1.5,
                    piston_velocity: 0.0,
                    hinge_angle: 0.0,
                    hinge_velocity: 0.0,
                    hinge_target: None,
                    free: false,
                    gyro_override: None,
                    door_open: false,
                },
                BlockInfo {
                    id: 3,
                    kind: BlockKind::Thruster,
                    name: "T1".into(),
                    forward: DVec3::NEG_Z,
                    up: DVec3::Y,
                    position: DVec3::X,
                    same_grid: true,
                    thrust_max: 30000.0,
                    thrust_override: 0.0,
                    thrust_current: 0.0,
                    piston_position: 0.0,
                    piston_velocity: 0.0,
                    hinge_angle: 0.0,
                    hinge_velocity: 0.0,
                    hinge_target: None,
                    free: false,
                    gyro_override: None,
                    door_open: false,
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn commands_and_print() {
        let src = r#"
            local arm = grid:block("Arm")
            print("init", arm.extension)
            function tick(dt)
              arm:set_velocity(0.5)
              for _, t in grid:blocks("thruster") do
                t:set_override(2)
                print(t.forward.z, t)
              end
            end
        "#;
        let (host, init) = ScriptHost::new(src, "", &view()).unwrap();
        assert_eq!(init.output, vec!["init\t1.500"]);
        let r = host.tick(view());
        assert_eq!(r.error, None);
        assert_eq!(r.commands, vec![Command::PistonVelocity(2, 0.5), Command::ThrusterOverride(3, 1.0)]);
        assert_eq!(r.output, vec!["-1\tthruster<T1>"]);
    }

    #[test]
    fn fields_are_not_cached() {
        let (host, _) = ScriptHost::new("function tick() print(grid.speed) end", "", &view()).unwrap();
        let mut v = view();
        v.grid.speed = 7.0;
        assert_eq!(host.tick(v).output, vec!["7"]);
    }

    #[test]
    fn infinite_loop_is_stopped() {
        let (host, _) = ScriptHost::new("function tick() while true do end end", "", &view()).unwrap();
        let r = host.tick(view());
        assert!(r.error.unwrap().contains("budget"));
        // The host keeps working afterwards.
        let r = host.tick(view());
        assert!(r.error.is_some());
    }

    #[test]
    fn memory_and_sandbox() {
        let (host, _) = ScriptHost::new(
            "function tick() local t = {} for i = 1, 1e9 do t[i] = string.rep('x', 1000) end end",
            "",
            &view(),
        )
        .unwrap();
        let r = host.tick(view());
        assert!(r.error.is_some());
        assert!(ScriptHost::new("os.exit()", "", &view()).is_err());
        assert!(ScriptHost::new("grid = nil", "", &view()).is_ok());
    }

    #[test]
    fn storage_persists_and_errors_are_reported() {
        let (host, _) = ScriptHost::new("storage = storage .. 'b'", "a", &view()).unwrap();
        assert_eq!(host.storage(), "ab");
        let e = ScriptHost::new("local x = ", "", &view()).err().unwrap();
        assert!(e.contains("script"), "{e}");
        let (host, _) = ScriptHost::new("function tick() grid:block('Arm'):set_override(1) end", "", &view()).unwrap();
        let r = host.tick(view());
        assert!(r.error.unwrap().contains("set_override called on a piston"));
    }
}
