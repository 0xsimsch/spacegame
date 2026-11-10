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

/// Three stacked pistons extend one after another, pause, then retract.
pub const ELEVATOR: &str = r#"-- Elevator: three stacked pistons extend one after another, pause, retract.
local lifts = { grid:block("Lift 1"), grid:block("Lift 2"), grid:block("Lift 3") }
assert(lifts[1] and lifts[2] and lifts[3], "needs pistons 'Lift 1'..'Lift 3'")
local speed = 2
local stage, dir, wait = 1, 1, 2
me:set_frequency(10)

function tick(dt)
  if wait > 0 then
    wait -= dt
    return
  end
  local p = lifts[stage]
  local done = if dir > 0 then p.extension >= p.max - 0.05 else p.extension <= p.min + 0.05
  if not done then
    p:set_velocity(speed * dir)
    return
  end
  p:set_velocity(0)
  if dir > 0 and stage < #lifts then
    stage += 1
  elseif dir < 0 and stage > 1 then
    stage -= 1
  else
    dir = -dir
    wait = 3
    print(if dir < 0 then "top reached" else "bottom reached")
  end
end

function on_message(arg)
  speed = math.clamp(tonumber(arg) or speed, 0.2, 2)
  print("speed", speed)
end
"#;

/// A luffing crane whose hook hinge counter-rotates to hang straight down.
pub const CRANE: &str = r#"-- Crane: slew around, raise/lower the boom, telescope it, keep the hook hanging straight down.
local slew = grid:block("Slew Rotor")
local luff = grid:block("Boom Hinge")
local boom = grid:block("Boom Piston")
local hook = grid:block("Hook Hinge")
assert(slew and luff and boom and hook, "needs 'Slew Rotor', 'Boom Hinge', 'Boom Piston' and 'Hook Hinge'")
local t = 0

function tick(dt)
  t += dt
  -- Slowly turn the whole tower (the rotor has no angle limits).
  slew:set_velocity(10)
  -- Negative angles lift the boom (right-hand rule around the hinge axis).
  luff:set_target_angle(-30 - 25 * math.sin(t * 0.25))
  local want = 2.5 + 2.4 * math.sin(t * 0.18)
  boom:set_velocity(math.clamp((want - boom.extension) * 2, -2, 2))
  -- The hook hinge turns around the opposite axis, so matching the boom
  -- angle cancels its tilt.
  hook:set_target_angle(luff.angle)
end

function on_message(arg)
  print(string.format("slew %.0f deg, boom %.1f deg, %.2f m out, hook %.1f deg", slew.angle, luff.angle, boom.extension, hook.angle))
end
"#;

/// A 4-joint robot arm following phase-shifted sine waves.
pub const ROBOT: &str = r#"-- Robot arm: shoulder, telescoping upper arm, elbow (other axis) and wrist.
local shoulder = grid:block("Shoulder")
local reach = grid:block("Reach Piston")
local elbow = grid:block("Elbow")
local wrist = grid:block("Wrist")
assert(shoulder and reach and elbow and wrist, "needs Shoulder, Reach Piston, Elbow, Wrist")
local t = 0
local speed = 1

function tick(dt)
  t += dt * speed
  shoulder:set_target_angle(30 * math.sin(t * 0.6))
  elbow:set_target_angle(55 * math.sin(t * 0.45 + 1))
  wrist:set_target_angle(70 * math.sin(t * 0.9 + 2))
  local want = 2.5 + 2 * math.sin(t * 0.5)
  reach:set_velocity(math.clamp((want - reach.extension) * 2, -2, 2))
end

function on_message(arg)
  speed = math.clamp(tonumber(arg) or 1, 0, 3)
  print("dance speed", speed)
end
"#;

/// A drawbridge that opens and closes on a timer or on command.
pub const BRIDGE: &str = r#"-- Drawbridge: cycles every 8 s. "Run" with "open", "close" or "auto".
local hinge = grid:block("Bridge Hinge")
assert(hinge, "needs a hinge named 'Bridge Hinge'")
local open, auto, timer = false, true, 0
me:set_frequency(10)

function on_message(arg)
  if arg == "open" then open, auto = true, false
  elseif arg == "close" then open, auto = false, false
  else auto = true end
  print("bridge:", if open then "open" else "closed", if auto then "(auto)" else "(manual)")
end

function tick(dt)
  timer += dt
  if auto and timer > 8 then
    timer = 0
    open = not open
  end
  hinge:set_target_angle(if open then 75 else 0)
end
"#;

/// A carousel: the rotor spins up and down, gondolas swing out like a real chair-o-plane.
pub const CAROUSEL: &str = r#"-- Carousel: spin up, slow down, reverse.
-- By default the swing hinges are FREE (motor off), so the gondolas swing out purely
-- from the physics of the spinning hub. Run with "motor" to drive them to the angle a
-- real swing ride would reach instead (tan(angle) = w^2 * r / g), "free" to release
-- them again, or a number to set the top speed (deg/s).
local rotor = grid:block("Carousel Rotor")
local swings = {}
for i = 1, 4 do
  swings[i] = grid:block("Swing " .. i)
  assert(swings[i], "needs hinges 'Swing 1'..'Swing 4'")
end
assert(rotor, "needs a rotor named 'Carousel Rotor'")
local top_speed = 90
local free = true
local t = 0
for _, s in swings do
  s:set_free(free)
end

function tick(dt)
  t += dt
  rotor:set_velocity(top_speed * math.sin(t * 0.15))
  if not free then
    local w = math.rad(rotor.velocity)
    local angle = math.deg(math.atan(w * w * 5 / 9.81))
    for _, s in swings do
      s:set_target_angle(math.min(angle, 75))
    end
  end
end

function on_message(arg)
  if arg == "free" or arg == "motor" then
    free = arg == "free"
    for _, s in swings do
      s:set_free(free)
    end
    print("swings:", if free then "free (physics)" else "motor-driven")
  else
    top_speed = math.clamp(tonumber(arg) or top_speed, 0, 180)
    print("top speed", top_speed, "deg/s")
  end
  print(string.format("rotor %.0f deg; swing 1 at %.1f deg", rotor.angle, swings[1].angle))
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

/// The shuttle's flight computer: takeoff, hover, landing, self-levelling, ramp/legs/doors.
pub fn shuttle_autopilot() -> String {
    format!(
        r#"-- Shuttle autopilot. "Run" with:
--   takeoff        close up, lift off, retract legs, hover at 40 m
--   hover <m>      hold an altitude
--   land           deploy legs, descend gently, touch down, open the ramp
--   level          only keep the shuttle level (fly the thrusters yourself)
--   off            manual flight from the cockpit
--   ramp | door    toggle the rear ramp / side door
{THRUST_HELPER}
local gyro = grid:block("Gyroscope")
local ramp = grid:block("Ramp Hinge")
local doors = {{ grid:block("Side Door Lower"), grid:block("Side Door Upper") }}
local legs = {{}}
for i = 1, 4 do legs[i] = grid:block("Leg " .. i) end
assert(gyro and ramp and legs[4], "needs Gyroscope, Ramp Hinge and Leg 1..4")

local mode = "off"
local target_alt = 40
local still = 0
local ramp_open = false

local function set_ramp(open)
  ramp_open = open
  ramp:set_target_angle(if open then -50 else 88)
end
local function set_doors(open)
  for _, d in doors do d:set_open(open) end
end
local leg_target = 1
local function set_legs(extended)
  leg_target = if extended then 1 else 0
end
-- Drive the legs to their target extension (called every tick).
local function update_legs()
  for _, l in legs do
    l:set_velocity(math.clamp((leg_target - l.extension) * 2, -1, 1))
  end
end
local function release()
  thrust_toward(vector.create(0, 0, 0))
  gyro:set_override(nil)
end

-- Turn rate (deg/s) that rotates the grid's up axis onto `want_up` (grid frame).
local function level_rates(want_up)
  local up = vector.create(0, 1, 0)
  local axis = vector.cross(up, want_up)
  local s = vector.magnitude(axis)
  local rates = vector.create(0, 0, 0)
  if s > 1e-4 then
    local angle = math.deg(math.asin(math.min(1, s)))
    rates = axis / s * math.min(angle * 1.5, 40)
  end
  -- Kill yaw drift as well (rotation around up).
  return vector.create(rates.x, 0, rates.z)
end

function on_message(arg)
  local cmd, value = string.match(arg, "^(%a+)%s*(%-?[%d%.]*)$")
  cmd = cmd or arg
  if cmd == "takeoff" then
    set_ramp(false); set_doors(false)
    mode, target_alt = "hover", 40
  elseif cmd == "hover" then
    mode, target_alt = "hover", tonumber(value) or target_alt
  elseif cmd == "land" then
    set_legs(true)
    mode, still = "land", 0
  elseif cmd == "level" or cmd == "off" then
    mode = cmd
    release()
  elseif cmd == "ramp" then
    set_ramp(not ramp_open)
  elseif cmd == "door" then
    set_doors(not doors[1].is_open)
  else
    print("unknown command", arg)
    return
  end
  print("autopilot:", mode, if mode == "hover" then target_alt .. " m" else "")
end

set_ramp(false)
set_legs(true)

function tick(dt)
  update_legs()
  if mode == "off" then return end
  local g = grid.gravity
  local gl = vector.magnitude(g)
  local up = if gl > 0.01 then -g / gl else vector.create(0, 1, 0)
  gyro:set_override(level_rates(up))
  if mode == "level" then return end

  local vs = vector.dot(grid.velocity, up)
  local horizontal = grid.velocity - up * vs
  local want_vs
  if mode == "hover" then
    want_vs = math.clamp((target_alt - grid.altitude) * 0.5, -6, 6)
    if grid.altitude > 12 then set_legs(false) end
  else -- land
    want_vs = -math.clamp((grid.altitude - 4) * 0.35, 0.8, 6)
    if math.abs(vs) < 0.3 and grid.altitude < 8 then
      still += dt
    else
      still = 0
    end
    if still > 1 then
      mode = "off"
      release()
      set_ramp(true)
      print("landed on", grid.planet)
      return
    end
  end
  thrust_toward(up * (gl + (want_vs - vs) * 1.5) - horizontal * 0.6)
end
"#
    )
}

pub fn all() -> Vec<(&'static str, String)> {
    vec![
        ("Hello", HELLO.to_string()),
        ("Arm sweep", ARM_SWEEP.to_string()),
        ("Elevator", ELEVATOR.to_string()),
        ("Crane", CRANE.to_string()),
        ("Robot arm", ROBOT.to_string()),
        ("Drawbridge", BRIDGE.to_string()),
        ("Carousel", CAROUSEL.to_string()),
        ("Shuttle autopilot", shuttle_autopilot()),
        ("Velocity hold", velocity_hold()),
        ("Hover", hover()),
    ]
}
