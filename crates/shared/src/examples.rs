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

/// The cruiser's computer: sensor-guided turrets and door control.
pub const CRUISER: &str = r#"-- Cruiser computer. The turrets track the nearest player outside the
-- hull that the sensor sees (300 m). "Run" with:
--   track | park        turrets follow targets / stand upright
--   airlocks | hangar   open or close the side airlocks / the hangar door in the prow
local sensor = grid:block("Cruiser Sensor")
assert(sensor, "needs a sensor named 'Cruiser Sensor'")
sensor:set_range(300)
-- Every "Turret N Rotor" / "Turret N Hinge" pair is a turret.
local turrets = {}
while true do
  local i = #turrets + 1
  local r, h = grid:block("Turret " .. i .. " Rotor"), grid:block("Turret " .. i .. " Hinge")
  if not (r and h) then break end
  turrets[i] = { rotor = r, hinge = h }
end
print(#turrets, "turrets online")
local airlocks, hangar = {}, {}
for _, d in grid:blocks("door") do
  table.insert(if string.find(d.name, "Hangar") then hangar else airlocks, d)
end
local tracking = true
local current = nil

-- Bounding box of the hull and its thrusters.
local function in_hull(p)
  return math.abs(p.x) < 16 and p.y > -2 and p.y < 19 and p.z > -44 and p.z < 39
end

local function pick()
  for _, p in sensor.players do
    if not in_hull(p.position) then return p end
  end
  return nil
end

local function toggle(list, what)
  if #list == 0 then print("no", what) return end
  local open = not list[1].is_open
  for _, d in list do d:set_open(open) end
  print(what, if open then "open" else "closed")
end

function on_message(arg)
  if arg == "track" or arg == "park" then
    tracking = arg == "track"
    print("turrets:", arg)
  elseif arg == "airlocks" then
    toggle(airlocks, "airlocks")
  elseif arg == "hangar" or arg == "cargo" then
    toggle(hangar, "hangar door")
  else
    print("unknown command", arg)
  end
end

function tick(dt)
  local p = if tracking then pick() else nil
  if (p and p.name) ~= current then
    current = p and p.name
    print(if current then "tracking " .. current else "no target")
  end
  for _, t in turrets do
    if p then
      -- Rotor: right-handed around +Y; hinge 0 = barrel up, +90 = level toward +Z.
      local d = p.position - (t.hinge.position + vector.create(0, 0.5, 0))
      local flat = math.sqrt(d.x * d.x + d.z * d.z)
      t.rotor:set_target_angle(math.deg(math.atan2(d.x, d.z)))
      t.hinge:set_target_angle(math.clamp(math.deg(math.atan2(flat, d.y)), 0, 90))
    else
      t.hinge:set_target_angle(0)
    end
  end
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

/// The follow drone: finds players with its sensor and flies after the nearest one.
pub fn drone_follow() -> String {
    format!(
        r#"-- Follow drone. It looks for players with its sensor and keeps station
-- a few metres behind and above the nearest one, facing them. "Run" with:
--   follow         follow the nearest player (default)
--   follow <name>  follow only that player
--   stay           hover where it is
--   off            switch the thrusters and gyroscope off (it drops!)
--   distance <m>   horizontal stand-off (default 5)
--   height <m>     height above the player (default 3)
{THRUST_HELPER}
local sensor = grid:block("Drone Sensor")
local gyro = grid:block("Drone Gyro")
assert(sensor and gyro, "needs a sensor named 'Drone Sensor' and a gyroscope named 'Drone Gyro'")
sensor:set_range(150)

local mode = "follow"
local target_name = nil
local distance, height = 5, 3
local max_speed = 30
local lost = 0

-- Gyroscope rates (deg/s) that level the drone and turn its front (-Z) toward `look`.
local function steer(up, look)
  local axis = vector.cross(vector.create(0, 1, 0), up)
  local s = vector.magnitude(axis)
  local rates = vector.create(0, 0, 0)
  if s > 1e-4 then
    rates = axis / s * math.min(math.deg(math.asin(math.min(1, s))) * 2, 60)
  end
  local yaw = 0
  if look then
    local h = look - up * vector.dot(look, up)
    if vector.magnitude(h) > 0.5 then
      local f = vector.create(0, 0, -1)
      h = vector.normalize(h)
      local angle = math.deg(math.atan2(vector.dot(vector.cross(f, h), up), vector.dot(f, h)))
      yaw = math.clamp(angle * 2, -90, 90)
    end
  end
  return vector.create(rates.x, yaw, rates.z)
end

local function pick()
  for _, p in sensor.players do
    if target_name == nil or string.lower(p.name) == target_name then
      return p
    end
  end
  return nil
end

function on_message(arg)
  local cmd, value = string.match(arg, "^(%a+)%s*(.*)$")
  cmd = cmd or arg
  if cmd == "follow" then
    mode = "follow"
    target_name = if value ~= "" then string.lower(value) else nil
  elseif cmd == "stay" or cmd == "off" then
    mode = cmd
    if cmd == "off" then
      thrust_toward(vector.create(0, 0, 0))
      gyro:set_override(nil)
    end
  elseif cmd == "distance" then
    distance = math.clamp(tonumber(value) or distance, 2, 50)
  elseif cmd == "height" then
    height = math.clamp(tonumber(value) or height, -10, 50)
  else
    print("unknown command", arg)
    return
  end
  print("drone:", mode, target_name or "", "distance", distance, "height", height)
end

function tick(dt)
  if mode == "off" then return end
  local g = grid.gravity
  local gl = vector.magnitude(g)
  local up = if gl > 0.01 then -g / gl else vector.create(0, 1, 0)
  local p = if mode == "follow" then pick() else nil
  if not p then
    if mode == "follow" then
      lost += dt
      if lost > 1 and lost - dt <= 1 then print("no player in sensor range") end
    end
    -- Hover in place.
    gyro:set_override(steer(up, nil))
    thrust_toward(-grid.velocity * 1.5 - g)
    return
  end
  if lost > 1 then print("following", p.name) end
  lost = 0
  -- Station point: `distance` from the player on our side, `height` above.
  local to = p.position
  local horiz = to - up * vector.dot(to, up)
  local hd = vector.magnitude(horiz)
  local away = if hd > 0.1 then -horiz / hd else vector.create(0, 0, 1)
  local goal = to + away * distance + up * height
  local want = goal * 0.9
  local wl = vector.magnitude(want)
  if wl > max_speed then want = want / wl * max_speed end
  want += p.velocity
  -- Never sink below 2 m above the ground.
  if gl > 0.01 and grid.altitude < 2 then
    want += up * (2 - grid.altitude) * 2
  end
  thrust_toward((want - grid.velocity) * 2 - g)
  gyro:set_override(steer(up, to))
end
"#
    )
}

/// Magnet crane: picks up a crate with a landing gear and carries it back and forth.
pub const MAGNET_CRANE: &str = r#"-- Magnet crane. A landing gear ("Magnet") on a piston picks up the crate,
-- the rotor swings it to the other side, sets it down and lets go; then it
-- picks it up again and carries it back. Run "stop" / "start".
local rotor = grid:block("Crane Rotor")
local piston = grid:block("Magnet Piston")
local magnet = grid:block("Magnet")
assert(rotor and piston and magnet, "needs Crane Rotor, Magnet Piston and Magnet")
local mode, timer = "lower", 0
local target, pick_ext = 0, 2
local running = true
rotor:set_target_angle(0)

local function set(m)
  mode, timer = m, 0
end

local function angle_error()
  return (rotor.angle - target + 540) % 360 - 180
end

function on_message(arg)
  running = arg ~= "stop"
  print(if running then "crane running" else "crane stopped")
end

function tick(dt)
  timer += dt
  if not running then piston:set_velocity(0) return end
  if mode == "lower" then
    piston:set_velocity(0.8)
    if magnet.locked then
      pick_ext = piston.extension
      print(string.format("magnet locked at %.2f m, lifting", pick_ext))
      set("lift")
    elseif piston.extension >= 2.6 then
      -- Stop above the ground (the magnet would lock onto the planet).
      print("nothing to pick up")
      set("lift")
    end
  elseif mode == "lift" then
    piston:set_velocity(-1)
    if piston.extension < 0.05 then
      if magnet.locked then
        target = if target == 0 then 180 else 0
        rotor:set_target_angle(target)
        set("swing")
      elseif timer > 1.5 then
        set("lower")
      end
    end
  elseif mode == "swing" then
    piston:set_velocity(0)
    if math.abs(angle_error()) < 1 and timer > 1 then set("set_down") end
  elseif mode == "set_down" then
    piston:set_velocity(0.6)
    if piston.extension >= pick_ext - 0.02 then
      magnet:unlock()
      print("set down at", target, "deg")
      set("lift")
    end
  end
end
"#;

/// Shared flight helpers for the demo vehicles: thrust allocation and levelling.
const FLIGHT_HELPER: &str = r#"
local function key(v)
  return string.format("%d,%d,%d", math.round(v.x), math.round(v.y), math.round(v.z))
end
-- Fire thrusters so the grid accelerates by `accel` (grid frame, m/s^2).
local function thrust_toward(accel)
  local thrusters = grid:blocks("thruster")
  local cap = {}
  for _, t in thrusters do
    cap[key(t.forward)] = (cap[key(t.forward)] or 0) + t.max_thrust
  end
  for _, t in thrusters do
    local f = math.max(0, vector.dot(t.forward, accel)) * grid.mass
    t:set_override(math.clamp(f / cap[key(t.forward)], 0, 1))
  end
end
-- Gyro rates (deg/s) that turn the grid's up axis onto `want_up`, without yaw.
local function level(want_up)
  local axis = vector.cross(vector.create(0, 1, 0), want_up)
  local s = vector.magnitude(axis)
  if s < 1e-4 then return vector.create(0, 0, 0) end
  local r = axis / s * math.min(math.deg(math.asin(math.min(1, s))) * 2, 40)
  return vector.create(r.x, 0, r.z)
end
-- Thrust that drives the vertical speed to `want_vs` and kills sideways drift.
local function fly(want_vs, hold_sideways)
  local g = grid.gravity
  local gl = vector.magnitude(g)
  local up = if gl > 0.01 then -g / gl else vector.create(0, 1, 0)
  local vs = vector.dot(grid.velocity, up)
  local side = grid.velocity - up * vs
  local a = up * (gl + (want_vs - vs) * 2)
  if hold_sideways then a -= side * 1.5 end
  thrust_toward(a)
  return up
end
"#;

/// Docking drone: parks on its pad with landing gear, flies up to the tower's
/// connector, docks, hangs there, undocks and lands again.
pub fn docking_drone() -> String {
    format!(
        r#"-- Docking drone. Cycle: parked on the pad (landing gear locked) ->
-- climb to the tower -> dock with the connector (thrusters off, it hangs) ->
-- undock -> descend -> the gear locks onto the pad again. Run "stop" / "start".
{FLIGHT_HELPER}
local gyro = grid:block("Gyroscope")
local port = grid:block("Docking Connector")
local gears = grid:blocks("landing_gear")
assert(gyro and port and #gears > 0, "needs Gyroscope, Docking Connector and landing gear")
local mode, timer = "parked", 0
local running = true

local function set(m, msg)
  mode, timer = m, 0
  if msg then print(msg) end
end

local function engines_off()
  thrust_toward(vector.create(0, 0, 0))
  gyro:set_override(nil)
end

local function landed()
  for _, g in gears do
    if g.locked then return true end
  end
  return false
end

function on_message(arg)
  running = arg ~= "stop"
  print(if running then "drone running" else "drone stopped after this cycle")
end

function tick(dt)
  timer += dt
  if mode == "parked" then
    engines_off()
    if running and timer > 4 then
      for _, g in gears do g:unlock() end
      set("climb", "gear unlocked, climbing to the dock")
    end
  elseif mode == "climb" then
    if port.connected then
      engines_off()
      set("docked", "docked - hanging from the connector")
      return
    end
    -- Slow down once the connector magnets have us.
    local up = fly(if port.ready then 0.2 else 1.5, not port.ready)
    gyro:set_override(level(up))
  elseif mode == "docked" then
    engines_off()
    if timer > 5 then
      port:disconnect()
      set("descend", "undocked, descending")
    end
  elseif mode == "descend" then
    if landed() then
      engines_off()
      set("parked", "landed - gear locked to the pad")
      return
    end
    local up = fly(-1.2, true)
    gyro:set_override(level(up))
  end
end
"#
    )
}

/// Merge module: rises into the gantry, merges into it, splits off again.
pub fn merge_module() -> String {
    format!(
        r#"-- Merge module. Cycle: rise under the gantry -> the merge blocks pull the
-- module into place and fuse it with the gantry (one grid!) -> after a few
-- seconds the module switches its merge block off, splits away and drops,
-- then switches it back on and rises again. Run "stop" / "start".
{FLIGHT_HELPER}
local gyro = grid:block("Gyroscope")
local merge = grid:block("Module Merge Block")
assert(gyro and merge, "needs Gyroscope and Module Merge Block")
local mode, timer = "rise", 0
local running = true

local function set(m, msg)
  mode, timer = m, 0
  if msg then print(msg) end
end

function on_message(arg)
  running = arg ~= "stop"
  print(if running then "module running" else "module stops after this cycle")
end

function tick(dt)
  timer += dt
  if mode == "merged" then
    -- We are part of the gantry now: nothing to fly.
    if running and timer > 4 then
      merge:set_enabled(false)
      set("drop", "merge block off - splitting away")
    end
    return
  end
  if merge.merged then
    thrust_toward(vector.create(0, 0, 0))
    gyro:set_override(nil)
    set("merged", "merged into the gantry")
    return
  end
  if mode == "rise" then
    -- Once the merge blocks are pulling, just carry our weight and let them work.
    local up = fly(if merge.ready then 0 else 1.2, not merge.ready)
    gyro:set_override(level(up))
  elseif mode == "drop" then
    local up = fly(if timer < 2 then -1.5 else 0, true)
    gyro:set_override(level(up))
    if timer > 5 then
      merge:set_enabled(true)
      set("rise", "merge block on - rising")
    end
  end
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
        ("Follow drone", drone_follow()),
        ("Cruiser computer", CRUISER.to_string()),
        ("Magnet crane", MAGNET_CRANE.to_string()),
        ("Docking drone", docking_drone()),
        ("Merge module", merge_module()),
        ("Velocity hold", velocity_hold()),
        ("Hover", hover()),
    ]
}
