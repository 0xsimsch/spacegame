# Spacegame prototype

Godot 4.7 client + Rust core. A scaled solar system, multiplayer, grid building,
pistons/hinges/thrusters and programmable blocks scripted in Luau.

## Run it

```sh
cargo build                      # builds the GDExtension (target/debug/libclient.so) + server
godot --path godot               # opens the game: "Host & Play" or "Join"
```

Dedicated server and test bots:

```sh
cargo run --release -p server -- [--port 47800] [--save world.save | --no-save] \
                                 [--latency MS] [--loss PCT] [--stress]
cargo run --release -p server --bin bot -- --addr 127.0.0.1:47800 --count 4 --seconds 30
```

Client command-line options (after `--`): `--host`, `--join HOST:PORT`, `--name N`,
`--port P`, `--autotest [--shots DIR] [--expect-players N]`.

Tests: `cargo test --workspace` (31 tests, including the three acceptance scripts) and
`godot --headless --path godot -- --autotest --host --port 47891` (end-to-end client smoke test).

## Controls

| Key | Action |
|---|---|
| WASD / Space / C | move (jetpack), Q/E roll in zero-g |
| J / Z / X | jetpack, inertial dampeners, cruise (speed cap 150 → 5000 m/s) |
| G | build mode: 1-9, 0 / wheel pick block (5 rotor, 8 gyroscope, 9 door, 0 sensor), R/T rotate, B ship/station, LMB place, RMB remove |
| F | sit in cockpit / open or close a door / edit programmable block / control panel of a block; F again to stand up |
| K | control panel of the grid you look at or sit in |
| Enter | chat and commands: `/help`, `/tp moon`, `/tpship mars`, `/spawn ship hover`, `/spawn <shuttle\|drone\|arm\|elevator\|crane\|robot\|bridge\|carousel>`, `/save`, `/where` |
| V | first / third person; in third person the mouse wheel zooms (seated: chase camera fixed to the seat, 3-300 m) |
| F1 / F3 / Esc | help, stats overlay, menu |

## Demo park

The default world, created when there is no save, places these next to the spawn. Each one is
driven by its own programmable block; open it with F to read or change the script.

| Construct | Mechanics | Script |
|---|---|---|
| Arm | piston → hinge → arm | sweeps in/out and ±60° |
| Elevator | 3 stacked pistons carrying a platform (15 m) | extends them one after another, pauses, retracts. Run with a number to set the speed |
| Crane | slew rotor → tower → luffing hinge → boom → telescoping piston → hook hinge | turns the tower, raises the boom and telescopes it; the hook counter-rotates to hang straight down |
| Robot arm | shoulder hinge → piston → elbow hinge (other axis) → wrist hinge → gripper | phase-shifted sine "dance". Run with a number to set the speed |
| Carousel | rotor → hub with 4 arms → swing hinges → gondolas with seats | spins up, slows, reverses; the swing hinges are free, so the gondolas swing out from real centrifugal force. Run `motor` to drive them to atan(ω²r/g) instead. Sit in one with F |
| Shuttle | 5×4×7 walk-in hull: cockpit, side door, rear ramp on a hinge, 4 piston landing legs, 18 thrusters on all six sides, gyroscope | the "Shuttle Computer" autopilot. Run with `takeoff`, `hover 60`, `land` (lands and opens the ramp), `level`, `off` (manual), `ramp`, `door`. It keeps itself level with the gyroscope |
| Drawbridge | wall → hinge → 3×6 deck | opens/closes every 8 s. Run with `open`, `close` or `auto` |
| Drone | quad layout: 4 corner lift thrusters, one thruster per other direction, gyroscope, sensor on top | the "Drone Computer" follows the nearest player its sensor sees (150 m), 5 m away and 3 m up, facing them. Run with `follow`, `follow <name>`, `stay`, `off`, `distance 8`, `height 5`. `/spawn drone` makes another |

Mechanical joints are Rapier multibody (reduced-coordinate) joints, so long chains stay stiff.
To regenerate the demo world, delete `world.save`: in `~/.local/share/godot/app_userdata/Spacegame Prototype/`
for Host & Play, or in the server's working directory for the dedicated server.

## Scripting API (Luau, sandboxed)

```lua
local arm = grid:block("Arm Piston")       -- by name; grid:blocks("thruster") lists by type
me:set_frequency(10)                       -- run tick() every 1, 10 or 100 ticks
function tick(dt)
  arm:set_velocity(1.5)                    -- piston m/s; hinge deg/s
  print(arm.extension, grid.altitude, grid.velocity)  -- vectors are in the grid frame
end
function on_message(arg) end               -- "Run" button / argument
```

Blocks: `thruster:set_override(0..1)`, `.max_thrust`, `.current_thrust`, `.forward`;
`piston:set_velocity`, `.extension`, `.min`, `.max`; `hinge:set_velocity`,
`hinge:set_target_angle(deg|nil)`, `.angle`; `rotor`: same as a hinge, but it spins without
limits around its forward axis, `.angle` wraps at ±180 and targets take the shortest way round.
Any piston/hinge/rotor: `set_free(true)` turns the motor off so the joint moves only from
external forces (limits still apply); `.free`. The control panel has the same switch.
Gyroscope: `gyro:set_override(vector)` sets turn rates in deg/s (grid frame), `set_override(nil)` turns it off.
Door: `door:open()`, `door:close()`, `door:toggle()`, `door:set_open(bool)`, `.is_open`. An open door has no collider.
Sensor: `sensor.players` lists the players in range, nearest first, as tables with `name`, `id`,
`position` (grid frame, relative to the grid origin), `velocity` (grid frame), `distance`, `seated`;
`sensor:closest()`, `.detected`, `.count`, `.range`, `sensor:set_range(1..500)` (default 50 m). Grid: `velocity`, `world_velocity`,
`angular_velocity`, `gravity`, `altitude`, `planet`, `mass`, `speed`, `position`,
`grid:set_cruise(bool)`. Also `print`, `storage` (persisted string), `vector.*`.
Limits per run: 50k interrupts / 5 ms, 4 MB memory; a failing run applies no commands.
Examples live in `crates/shared/src/examples.rs` and the editor's "Load example" menu.

## Layout

```
crates/shared   protocol, block defs, 24 orientations, solar system, example scripts
crates/script   Luau sandbox: snapshot view in -> commands out (backend-agnostic API)
crates/sim      authoritative world: rapier3d-f64 physics, grids, joints, cockpit, scripts, save/load
crates/server   renet server: interest management, snapshots, latency/loss sim, commands; bot binary
crates/client   GDExtension: networking, interpolation, prediction, floating origin, scaled space, UI bridge
godot/          project, main.gd (menu, HUD, chat, control panel, script editor), autotest.gd
```

Key design points:

- **Coordinates**: the server simulates in `f64`. The client renders relative to the camera
  (floating origin), so stock single-precision Godot works with no custom engine build.
- **Planets**: bodies farther than 150 km are drawn in scaled space. The nearest body below
  40 km gets a high-resolution surface patch generated each frame in camera-relative coordinates.
- **Scripts**: each run reads an immutable snapshot and emits commands. Scripts run in parallel
  (rayon) and the commands are applied together; a script can only command blocks of its own
  mechanical group.
- **Network**: server-authoritative over renet/UDP. Snapshots go out at 30 Hz, other grids are
  interpolated 100 ms in the past, and the local player is predicted and reconciled. Each client
  only receives grids and players within 20 km.

## Milestone status

| | Result |
|---|---|
| M0 | Workspace, server, client connect, players see each other |
| M1 | Scaled solar system (10 bodies), f64 + floating origin, scaled space, jetpack/walking, gravity, `/tp` |
| M2 | Build/remove blocks, new ships or stations, compound colliders, replication |
| M3 | Thrusters, cockpit with dampeners and gyro, pistons and hinges with sub-grids and joint motors |
| M4 | Programmable block, Luau sandbox, editor UI; arm sweep, velocity hold and hover pass as tests |
| M5 | Save/load + autosave, own-player prediction, F3 stats, stress test: 4 players, 115 grids, 52 scripts at ~1.6 ms per tick |

## Known limitations (deliberately out of scope for the prototype)

- Planets do not orbit. There is no grid splitting, damage, power or inventory.
- One box collider per block (no box merging); one mesh node per block (no MultiMesh yet).
- Prediction ignores collisions (it is corrected by the server state on every snapshot).
- Block default names are numbered per grid, so sub-grids can repeat names (rename them in the panel).
