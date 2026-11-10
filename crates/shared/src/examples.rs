//! Example scripts, shown in the in-game editor and used by the prefabs/tests.

pub const HELLO: &str = r#"-- Runs once when compiled:
print("Hello from " .. me.name)
for _, b in grid:blocks() do
  print(b.type, b.name)
end

-- Runs every tick (see me:set_frequency):
function tick(dt)
end

-- Runs when someone presses "Run" with an argument:
function on_message(arg)
  print("got", arg)
end
"#;

/// Acceptance script 1: a piston extends/retracts while a hinge swings ±60°.
pub const ARM_SWEEP: &str = r#"-- Arm sweep: piston in/out, hinge swinging between -60 and +60 degrees.
local arm = grid:block("Arm Piston")
local elbow = grid:block("Elbow Hinge")
assert(arm and elbow, "needs blocks named 'Arm Piston' and 'Elbow Hinge'")
local out = true
local swing = 60
me:set_frequency(10)

function tick(dt)
  if out and arm.extension >= arm.max - 0.1 then
    out = false
  elseif not out and arm.extension <= arm.min + 0.1 then
    out = true
  end
  arm:set_velocity(if out then 1.5 else -1.5)
  if math.abs(elbow.angle - swing) < 3 then
    swing = -swing
  end
  elbow:set_target_angle(swing)
end

function on_message(arg)
  print("extension", arm.extension, "angle", elbow.angle)
end
"#;

/// Shared thruster-allocation helper used by the flight scripts.
const THRUST_HELPER: &str = r#"
-- Fire thrusters so the grid accelerates by `accel` (grid frame, m/s^2).
local function key(v)
  return string.format("%d,%d,%d", math.round(v.x), math.round(v.y), math.round(v.z))
end
local function thrust_toward(accel)
  local thrusters = grid:blocks("thruster")
  local cap = {}
  for _, t in thrusters do
    if t.same_grid then
      cap[key(t.forward)] = (cap[key(t.forward)] or 0) + t.max_thrust
    end
  end
  for _, t in thrusters do
    if t.same_grid then
      local f = math.max(0, vector.dot(t.forward, accel)) * grid.mass
      t:set_override(math.clamp(f / cap[key(t.forward)], 0, 1))
    end
  end
end
"#;

/// Acceptance script 2: hold 50 m/s forward and cancel sideways drift.
pub fn velocity_hold() -> String {
    format!(
        r#"-- Velocity hold: keep a forward speed (grid -Z) and kill sideways drift.
-- Send a number with "Run" to change the speed.
{THRUST_HELPER}
local speed = 50

function on_message(arg)
  speed = tonumber(arg) or speed
  print("target speed", speed)
end

function tick(dt)
  local target = vector.create(0, 0, -speed)
  local err = target - grid.velocity
  thrust_toward(err * 0.8 - grid.gravity)
end
"#
    )
}

/// Acceptance script 3: hover at a fixed altitude using gravity information.
pub fn hover() -> String {
    format!(
        r#"-- Hover: hold an altitude above the nearest body with thrusters only.
-- Send a number with "Run" to change the altitude.
{THRUST_HELPER}
local target = 25

function on_message(arg)
  target = tonumber(arg) or target
  print("target altitude", target)
end

function tick(dt)
  local g = grid.gravity
  local gl = vector.magnitude(g)
  if gl < 0.001 then
    thrust_toward(-grid.velocity)
    return
  end
  local up = -g / gl
  local vs = vector.dot(grid.velocity, up)
  local climb = math.clamp((target - grid.altitude) * 0.6 - vs * 1.2, -4, 4)
  -- Cancel horizontal drift as well.
  local horizontal = grid.velocity - up * vs
  thrust_toward(up * (gl + climb) - horizontal * 0.5)
end
"#
    )
}

pub fn all() -> Vec<(&'static str, String)> {
    vec![
        ("Hello", HELLO.to_string()),
        ("Arm sweep", ARM_SWEEP.to_string()),
        ("Velocity hold", velocity_hold()),
        ("Hover", hover()),
    ]
}
