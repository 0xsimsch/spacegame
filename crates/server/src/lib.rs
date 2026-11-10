//! Dedicated / embedded game server: renet networking around the `sim` world.

use rand::Rng;
use renet::{ClientId, ConnectionConfig, DefaultChannel, RenetServer, ServerEvent};
use renet_netcode::{NetcodeServerTransport, ServerAuthentication, ServerConfig as NetcodeConfig};
use shared::protocol::*;
use shared::solar::{body_by_name, gravity_at};
use shared::{BlockId, DVec3, GridId, INTEREST_RADIUS, PlayerId, SNAPSHOT_EVERY, TICK_DT, decode, encode};
use sim::{BlockState, World, WorldEvent, prefabs};
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub bind: SocketAddr,
    pub save_path: Option<PathBuf>,
    pub autosave: Duration,
    /// Artificial extra latency added to every outgoing message.
    pub sim_latency: Duration,
    /// Percentage (0-100) of unreliable messages to drop.
    pub sim_loss: f32,
    /// Spawn a stress-test scene (20 ships + 30 arms = 50 scripts).
    pub stress: bool,
    pub max_clients: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([0, 0, 0, 0], shared::DEFAULT_PORT)),
            save_path: Some(PathBuf::from("world.save")),
            autosave: Duration::from_secs(300),
            sim_latency: Duration::ZERO,
            sim_loss: 0.0,
            stress: false,
            max_clients: 16,
        }
    }
}

struct Client {
    player: Option<PlayerId>,
    name: String,
    known_grids: HashSet<GridId>,
    known_players: HashSet<PlayerId>,
    /// Outgoing messages held back to simulate latency.
    delayed: VecDeque<(Instant, u8, Vec<u8>)>,
    last_ack: u32,
}

pub struct GameServer {
    pub world: World,
    pub config: ServerConfig,
    renet: RenetServer,
    transport: NetcodeServerTransport,
    clients: HashMap<ClientId, Client>,
    next_player: PlayerId,
    last_save: Instant,
    tick_ms_avg: f32,
}

const UNRELIABLE: u8 = 0;
const RELIABLE: u8 = 1;

fn connection_config() -> ConnectionConfig {
    ConnectionConfig { available_bytes_per_tick: 250_000, ..Default::default() }
}

pub fn client_connection_config() -> ConnectionConfig {
    connection_config()
}

impl GameServer {
    pub fn new(config: ServerConfig) -> std::io::Result<Self> {
        let socket = UdpSocket::bind(config.bind)?;
        let public = socket.local_addr()?;
        let nc = NetcodeConfig {
            current_time: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap(),
            max_clients: config.max_clients,
            protocol_id: shared::PROTOCOL_ID,
            public_addresses: vec![public],
            authentication: ServerAuthentication::Unsecure,
        };
        let transport = NetcodeServerTransport::new(nc, socket)?;
        let mut world = match config.save_path.as_ref().and_then(|p| std::fs::read(p).ok()) {
            Some(bytes) => match World::load_bytes(&bytes) {
                Ok(w) => {
                    log::info!("loaded world: {} grids, {} blocks", w.grids.len(), w.total_blocks());
                    w
                }
                Err(e) => {
                    log::error!("could not load save ({e}); starting a new world");
                    World::new()
                }
            },
            None => World::new(),
        };
        if world.grids.is_empty() {
            seed_world(&mut world);
        }
        if config.stress {
            spawn_stress_scene(&mut world);
        }
        world.drain_events();
        Ok(Self {
            world,
            config,
            renet: RenetServer::new(connection_config()),
            transport,
            clients: HashMap::new(),
            next_player: 1,
            last_save: Instant::now(),
            tick_ms_avg: 0.0,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.transport.addresses()[0]
    }

    pub fn save(&self) {
        let Some(path) = &self.config.save_path else { return };
        let bytes = self.world.save_bytes();
        let tmp = path.with_extension("save.tmp");
        match std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, path)) {
            Ok(()) => log::info!("saved world to {} ({} KiB)", path.display(), bytes.len() / 1024),
            Err(e) => log::error!("save failed: {e}"),
        }
    }

    /// Run until `stop` is set. Saves on exit.
    pub fn run(&mut self, stop: Arc<AtomicBool>) {
        let tick = Duration::from_secs_f64(TICK_DT);
        let mut next = Instant::now();
        let mut last = Instant::now();
        let mut report = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            let now = Instant::now();
            let dt = now - last;
            last = now;
            self.update(dt);
            next += tick;
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            } else if now - next > Duration::from_millis(250) {
                next = now; // fell far behind: don't try to catch up
            }
            if report.elapsed() > Duration::from_secs(10) {
                report = Instant::now();
                let s = self.world.stats;
                log::info!(
                    "tick avg {:.2} ms (scripts {:.2}, physics {:.2}) | {} players, {} grids, {} blocks, {} scripts",
                    self.tick_ms_avg,
                    s.script_ms,
                    s.physics_ms,
                    self.world.players.len(),
                    self.world.grids.len(),
                    self.world.total_blocks(),
                    self.world.program_count()
                );
            }
        }
        for (id, _) in self.clients.iter() {
            self.renet.disconnect(*id);
        }
        self.transport.send_packets(&mut self.renet);
        self.save();
    }

    /// One server tick: network in, simulate, network out.
    pub fn update(&mut self, dt: Duration) {
        self.renet.update(dt);
        if let Err(e) = self.transport.update(dt, &mut self.renet) {
            log::warn!("transport: {e}");
        }
        while let Some(ev) = self.renet.get_event() {
            match ev {
                ServerEvent::ClientConnected { client_id } => {
                    log::info!("client {client_id} connected");
                    self.clients.insert(
                        client_id,
                        Client {
                            player: None,
                            name: String::new(),
                            known_grids: HashSet::new(),
                            known_players: HashSet::new(),
                            delayed: VecDeque::new(),
                            last_ack: 0,
                        },
                    );
                }
                ServerEvent::ClientDisconnected { client_id, reason } => {
                    log::info!("client {client_id} disconnected: {reason}");
                    if let Some(c) = self.clients.remove(&client_id) {
                        if let Some(p) = c.player {
                            self.world.remove_player(p);
                            self.broadcast(RELIABLE, &ServerMsg::PlayerLeft { id: p });
                            self.broadcast_chat("server", &format!("{} left", c.name));
                        }
                    }
                }
            }
        }
        let ids: Vec<ClientId> = self.clients.keys().copied().collect();
        for id in &ids {
            while let Some(bytes) = self.renet.receive_message(*id, DefaultChannel::Unreliable) {
                if let Some(msg) = decode::<ClientMsg>(&bytes) {
                    self.handle(*id, msg);
                }
            }
            while let Some(bytes) = self.renet.receive_message(*id, DefaultChannel::ReliableOrdered) {
                if let Some(msg) = decode::<ClientMsg>(&bytes) {
                    self.handle(*id, msg);
                }
            }
        }

        self.world.step();
        let s = self.world.stats;
        self.tick_ms_avg = self.tick_ms_avg * 0.95 + s.tick_ms * 0.05;
        let events = self.world.drain_events();
        self.dispatch_events(events);
        self.update_interest();
        if self.world.tick % SNAPSHOT_EVERY == 0 {
            self.send_snapshots();
        }
        if self.world.tick % 60 == 0 {
            let stats = ServerStats {
                tick_ms: self.tick_ms_avg,
                script_ms: s.script_ms,
                physics_ms: s.physics_ms,
                grids: self.world.grids.len() as u32,
                blocks: self.world.total_blocks() as u32,
                scripts: self.world.program_count() as u32,
                players: self.world.players.len() as u32,
            };
            self.broadcast(RELIABLE, &ServerMsg::Stats(stats));
        }
        if self.last_save.elapsed() > self.config.autosave {
            self.last_save = Instant::now();
            self.save();
        }
        self.flush_delayed();
        self.transport.send_packets(&mut self.renet);
    }

    // ------------------------------------------------------------- sending

    fn send(&mut self, client: ClientId, channel: u8, msg: &ServerMsg) {
        if !self.renet.is_connected(client) {
            return;
        }
        let bytes = encode(msg);
        let Some(c) = self.clients.get_mut(&client) else { return };
        if channel == UNRELIABLE && self.config.sim_loss > 0.0 {
            if rand::rng().random_range(0.0..100.0) < self.config.sim_loss {
                return;
            }
        }
        if self.config.sim_latency.is_zero() {
            self.renet.send_message(client, channel, bytes);
        } else {
            c.delayed.push_back((Instant::now() + self.config.sim_latency, channel, bytes));
        }
    }

    fn flush_delayed(&mut self) {
        let now = Instant::now();
        for (id, c) in self.clients.iter_mut() {
            while c.delayed.front().is_some_and(|(t, _, _)| *t <= now) {
                let (_, ch, bytes) = c.delayed.pop_front().unwrap();
                if self.renet.is_connected(*id) {
                    self.renet.send_message(*id, ch, bytes);
                }
            }
        }
    }

    fn joined(&self) -> Vec<ClientId> {
        self.clients.iter().filter(|(_, c)| c.player.is_some()).map(|(id, _)| *id).collect()
    }

    fn broadcast(&mut self, channel: u8, msg: &ServerMsg) {
        for id in self.joined() {
            self.send(id, channel, msg);
        }
    }

    /// Send to everyone who currently knows `grid`.
    fn send_grid_watchers(&mut self, grid: GridId, msg: &ServerMsg) {
        let ids: Vec<ClientId> = self.clients.iter().filter(|(_, c)| c.known_grids.contains(&grid)).map(|(id, _)| *id).collect();
        for id in ids {
            self.send(id, RELIABLE, msg);
        }
    }

    fn broadcast_chat(&mut self, from: &str, text: &str) {
        self.broadcast(RELIABLE, &ServerMsg::Chat { from: from.into(), text: text.into() });
    }

    fn notice(&mut self, client: ClientId, text: impl Into<String>) {
        self.send(client, RELIABLE, &ServerMsg::Chat { from: "server".into(), text: text.into() });
    }

    fn grid_data(&self, id: GridId) -> Option<GridData> {
        let g = self.world.grids.get(&id)?;
        let p = self.world.phys.bodies[g.body].position();
        let mut blocks: Vec<BlockData> = g.blocks.values().map(|b| b.data()).collect();
        blocks.sort_by_key(|b| b.id);
        Some(GridData { id, is_static: g.is_static, pos: p.translation, rot: p.rotation, blocks })
    }

    fn dispatch_events(&mut self, events: Vec<WorldEvent>) {
        for ev in events {
            match ev {
                // New grids reach clients through interest management.
                WorldEvent::GridAdded(_) => {}
                WorldEvent::GridRemoved(g) => {
                    let ids: Vec<ClientId> = self.clients.iter().filter(|(_, c)| c.known_grids.contains(&g)).map(|(id, _)| *id).collect();
                    for id in ids {
                        self.clients.get_mut(&id).unwrap().known_grids.remove(&g);
                        self.send(id, RELIABLE, &ServerMsg::GridRemoved(g));
                    }
                }
                WorldEvent::BlockAdded { grid, block } => {
                    self.send_grid_watchers(grid, &ServerMsg::BlockAdded { grid, block: block.clone() });
                    if let Some(info) = self.world.control_info(block.id) {
                        self.send_grid_watchers(grid, &ServerMsg::BlockControl(info));
                    }
                }
                WorldEvent::BlockRemoved { grid, block } => self.send_grid_watchers(grid, &ServerMsg::BlockRemoved { grid, block }),
                WorldEvent::BlockRenamed { grid, block, name } => self.send_grid_watchers(grid, &ServerMsg::BlockRenamed { block, name }),
                WorldEvent::BlockControl { grid, info } => self.send_grid_watchers(grid, &ServerMsg::BlockControl(info)),
                WorldEvent::ScriptOutput { grid, block, lines, error } => {
                    self.send_grid_watchers(grid, &ServerMsg::ScriptOutput { block, lines, error })
                }
                WorldEvent::Notice { player, text } => {
                    if let Some(id) = self.client_of(player) {
                        self.notice(id, text);
                    }
                }
            }
        }
    }

    fn client_of(&self, player: PlayerId) -> Option<ClientId> {
        self.clients.iter().find(|(_, c)| c.player == Some(player)).map(|(id, _)| *id)
    }

    /// Send grids entering a client's interest radius, drop those leaving it.
    fn update_interest(&mut self) {
        // Every 10 ticks is plenty for 20 km bubbles.
        if self.world.tick % 10 != 1 {
            return;
        }
        let grid_pos: Vec<(GridId, DVec3)> =
            self.world.grids.values().map(|g| (g.id, self.world.phys.bodies[g.body].translation())).collect();
        for id in self.joined() {
            let pid = self.clients[&id].player.unwrap();
            let Some(pp) = self.world.player_pos(pid) else { continue };
            let in_range: HashSet<GridId> = grid_pos
                .iter()
                .filter(|(_, p)| (*p - pp).length() < INTEREST_RADIUS)
                .map(|(g, _)| *g)
                .collect();
            let known = self.clients[&id].known_grids.clone();
            let leaving: Vec<GridId> =
                known.iter().filter(|g| !in_range.contains(g) && self.world.grids.contains_key(g)).copied().collect();
            let mut entering: Vec<GridId> = in_range.difference(&known).copied().collect();
            entering.sort();
            for g in leaving {
                self.clients.get_mut(&id).unwrap().known_grids.remove(&g);
                self.send(id, RELIABLE, &ServerMsg::GridRemoved(g));
            }
            for g in entering {
                if let Some(data) = self.grid_data(g) {
                    let terminal: Vec<BlockId> = data.blocks.iter().filter(|b| b.kind.is_terminal()).map(|b| b.id).collect();
                    self.clients.get_mut(&id).unwrap().known_grids.insert(g);
                    self.send(id, RELIABLE, &ServerMsg::FullGrid(data));
                    for b in terminal {
                        if let Some(info) = self.world.control_info(b) {
                            self.send(id, RELIABLE, &ServerMsg::BlockControl(info));
                        }
                    }
                }
            }
        }
    }

    fn send_snapshots(&mut self) {
        let players: Vec<(PlayerState, DVec3)> = self
            .world
            .players
            .values()
            .map(|p| {
                let b = &self.world.phys.bodies[p.body];
                (
                    PlayerState {
                        id: p.id,
                        pos: b.translation(),
                        vel: b.linvel(),
                        look: p.input.look,
                        seat: p.seat,
                        grounded: p.grounded,
                    },
                    b.translation(),
                )
            })
            .collect();
        for id in self.joined() {
            let c = &self.clients[&id];
            let pid = c.player.unwrap();
            let Some(pp) = self.world.player_pos(pid) else { continue };
            let mut snap = Snapshot {
                tick: self.world.tick,
                ack_input: self.world.players.get(&pid).map(|p| p.input.seq).unwrap_or(0),
                players: players.iter().filter(|(_, pos)| (*pos - pp).length() < INTEREST_RADIUS).map(|(s, _)| s.clone()).collect(),
                grids: Vec::with_capacity(c.known_grids.len()),
                values: Vec::new(),
            };
            for gid in &c.known_grids {
                let Some(g) = self.world.grids.get(gid) else { continue };
                let b = &self.world.phys.bodies[g.body];
                snap.grids.push(GridState {
                    id: *gid,
                    pos: b.translation(),
                    rot: *b.rotation(),
                    linvel: b.linvel().as_vec3(),
                    angvel: b.angvel().as_vec3(),
                });
                for blk in g.blocks.values() {
                    let v = match &blk.state {
                        BlockState::Thruster(t) => t.output,
                        BlockState::Mech(m) => m.value,
                        BlockState::Door { open } => *open as u8 as f64,
                        _ => continue,
                    };
                    snap.values.push(BlockValue { id: blk.id, value: v as f32 });
                }
            }
            let new_players: Vec<PlayerId> =
                snap.players.iter().map(|p| p.id).filter(|p| !self.clients[&id].known_players.contains(p)).collect();
            for p in new_players {
                if let Some(name) = self.world.players.get(&p).map(|pl| pl.name.clone()) {
                    self.clients.get_mut(&id).unwrap().known_players.insert(p);
                    self.send(id, RELIABLE, &ServerMsg::PlayerInfo { id: p, name });
                }
            }
            self.send(id, UNRELIABLE, &ServerMsg::Snapshot(snap));
        }
    }

    // ------------------------------------------------------------ receiving

    fn handle(&mut self, client: ClientId, msg: ClientMsg) {
        let player = self.clients.get(&client).and_then(|c| c.player);
        let Some(pid) = player else {
            if let ClientMsg::Hello { name } = msg {
                self.handle_hello(client, name);
            }
            return;
        };
        let result: Result<(), String> = match msg {
            ClientMsg::Hello { .. } => Ok(()),
            ClientMsg::Input(input) => {
                self.clients.get_mut(&client).unwrap().last_ack = input.seq;
                self.world.set_input(pid, input);
                Ok(())
            }
            ClientMsg::PlaceBlock { grid, cell, rot, kind } => self.world.place_block(pid, grid, cell, rot, kind).map(|_| ()),
            ClientMsg::RemoveBlock { grid, cell } => self.world.remove_block(pid, grid, cell),
            ClientMsg::NewGrid { pos, rot, kind, is_static } => self.world.new_grid(pid, pos, rot, kind, is_static).map(|_| ()),
            ClientMsg::RenameBlock { block, name } => self.world.rename_block(block, &name),
            ClientMsg::SetBlockProp { block, prop } => self.world.set_block_prop(block, prop),
            ClientMsg::RequestScript { block } => {
                if let Some(info) = self.world.script_info(block) {
                    self.send(client, RELIABLE, &ServerMsg::ScriptInfo(info));
                }
                Ok(())
            }
            ClientMsg::UploadScript { block, source } => {
                let r = self.world.upload_script(block, &source);
                if let Some(info) = self.world.script_info(block) {
                    self.send(client, RELIABLE, &ServerMsg::ScriptInfo(info));
                }
                r.map_err(|e| format!("compile error: {e}"))
            }
            ClientMsg::RunScript { block, arg } => self.world.run_script(block, &arg),
            ClientMsg::EnterSeat { block } => self.world.enter_seat(pid, block),
            ClientMsg::ExitSeat => {
                self.world.exit_seat(pid);
                Ok(())
            }
            ClientMsg::Chat(text) => {
                if let Some(cmd) = text.strip_prefix('/') {
                    self.command(client, pid, cmd)
                } else {
                    let name = self.clients[&client].name.clone();
                    let text: String = text.chars().take(300).collect();
                    self.broadcast_chat(&name, &text);
                    Ok(())
                }
            }
        };
        if let Err(e) = result {
            self.notice(client, e);
        }
    }

    fn handle_hello(&mut self, client: ClientId, name: String) {
        let mut name: String = name.trim().chars().filter(|c| !c.is_control()).take(24).collect();
        if name.is_empty() {
            name = format!("Player{client}");
        }
        while self.clients.values().any(|c| c.name == name) {
            name.push('_');
        }
        let pid = self.next_player;
        self.next_player += 1;
        self.world.add_player(pid, &name);
        let c = self.clients.get_mut(&client).unwrap();
        c.player = Some(pid);
        c.name = name.clone();
        log::info!("{name} joined as player {pid}");
        self.send(client, RELIABLE, &ServerMsg::Welcome { player: pid, tick: self.world.tick });
        self.broadcast_chat("server", &format!("{name} joined"));
        self.notice(client, "Welcome! Type /help for commands.");
    }

    fn command(&mut self, client: ClientId, pid: PlayerId, cmd: &str) -> Result<(), String> {
        let mut parts = cmd.split_whitespace();
        let name = parts.next().unwrap_or("");
        let args: Vec<&str> = parts.collect();
        match name {
            "help" => {
                self.notice(
                    client,
                    "/tp <body> - teleport (bodies: Sun Mercury Venus Earth Moon Mars Jupiter Saturn Uranus Neptune)\n\
                     /tpship <body> - teleport the ship you are seated in\n\
                     /spawn <ship|shuttle|arm|pad|elevator|crane|robot|bridge|carousel> [hover|velocity] - spawn a prefab in front of you\n\
                     /save - save the world\n/where - show your position",
                );
                Ok(())
            }
            "tp" | "tpship" => {
                let target = args.first().ok_or("usage: /tp <body>")?;
                let (_, body) = body_by_name(target).ok_or("unknown body")?;
                let dir = shared::solar::sunny_up(body);
                let alt = if name == "tp" { 30.0 } else { 80.0 };
                let dest = body.position + dir * (body.radius + alt);
                if name == "tp" {
                    self.world.teleport_player(pid, dest);
                } else {
                    let seat = self.world.players[&pid].seat.ok_or("you are not seated")?;
                    let (gid, _) = self.world.block_loc[&seat];
                    self.world.teleport_group(gid, dest);
                }
                self.notice(client, format!("teleported to {}", body.name));
                Ok(())
            }
            "spawn" => {
                let what = args.first().copied().unwrap_or("ship");
                if !prefabs::NAMES.contains(&what) {
                    return Err(format!("unknown prefab; try one of {:?}", prefabs::NAMES));
                }
                let script = match args.get(1).copied() {
                    Some("hover") => Some(shared::examples::hover()),
                    Some("velocity") => Some(shared::examples::velocity_hold()),
                    Some(other) => return Err(format!("unknown script '{other}'")),
                    None => None,
                };
                let p = self.world.players[&pid].input.look;
                let pos = self.world.player_pos(pid).unwrap();
                let fwd = p * DVec3::NEG_Z;
                let dist = match what {
                    "ship" | "pad" | "arm" => 8.0,
                    "shuttle" => 12.0,
                    _ => 15.0,
                };
                let at = pos + fwd * dist;
                let rot = prefabs::surface_orientation(at);
                let g = gravity_at(at);
                // On a planet put it on the surface below the target point.
                let at = if let Some(b) = g.body {
                    let body = &shared::solar::BODIES[b];
                    let up = (at - body.position).normalize();
                    let ground = body.position + up * (body.radius + match what {
                        "ship" => 1.6,
                        "shuttle" => 4.0,
                        _ => 0.6,
                    });
                    // Big constructs need some room in front of the player.
                    if g.altitude < 20.0 { ground } else { at }
                } else {
                    at
                };
                self.world.spawn_prefab(what, at, rot, script.as_deref());
                Ok(())
            }
            "save" => {
                self.save();
                self.notice(client, "world saved");
                Ok(())
            }
            "where" => {
                let pos = self.world.player_pos(pid).unwrap();
                let g = gravity_at(pos);
                let near = shared::solar::BODIES[g.nearest].name;
                self.notice(client, format!("{:.0} m above {near}; position {:.0}", g.altitude, pos));
                Ok(())
            }
            _ => Err(format!("unknown command /{name}")),
        }
    }
}

/// Starter content near the Earth spawn.
fn seed_world(world: &mut World) {
    let (_, earth) = body_by_name("Earth").unwrap();
    let spawn = earth.position + shared::solar::sunny_up(earth) * earth.radius;
    let rot = prefabs::surface_orientation(spawn);
    world.spawn_prefab("pad", spawn + rot * (DVec3::new(0.0, 0.6, -15.0)), rot, None);
    world.spawn_prefab("ship", spawn + rot * DVec3::new(0.0, 2.1, -15.0), rot, None);
    world.spawn_prefab("arm", spawn + rot * DVec3::new(12.0, 0.6, -10.0), rot, None);
    // Demo park of mechanical constructs, each driven by its own script.
    world.spawn_prefab("elevator", spawn + rot * DVec3::new(-12.0, 0.6, -12.0), rot, None);
    world.spawn_prefab("crane", spawn + rot * DVec3::new(-16.0, 0.6, -32.0), rot, None);
    world.spawn_prefab("robot", spawn + rot * DVec3::new(14.0, 0.6, -28.0), rot, None);
    world.spawn_prefab("bridge", spawn + rot * DVec3::new(0.0, 0.6, -42.0), rot, None);
    world.spawn_prefab("carousel", spawn + rot * DVec3::new(32.0, 0.6, -14.0), rot, None);
    // The shuttle's origin is its floor; its legs reach 3.5 m below.
    world.spawn_prefab("shuttle", spawn + rot * DVec3::new(-28.0, 4.0, 2.0), rot, None);
}

/// 20 hovering ships + 30 sweeping arms = 50 scripts, all near the Earth spawn.
pub fn spawn_stress_scene(world: &mut World) {
    let (_, earth) = body_by_name("Earth").unwrap();
    let spawn = earth.position + shared::solar::sunny_up(earth) * earth.radius;
    let rot = prefabs::surface_orientation(spawn);
    for i in 0..20 {
        let x = (i % 5) as f64 * 12.0 - 24.0;
        let z = (i / 5) as f64 * 12.0 + 30.0;
        world.spawn_prefab("ship", spawn + rot * DVec3::new(x, 1.6, z), rot, Some(&shared::examples::hover()));
    }
    for i in 0..30 {
        let x = (i % 6) as f64 * 8.0 - 20.0;
        let z = -(i / 6) as f64 * 8.0 - 40.0;
        world.spawn_prefab("arm", spawn + rot * DVec3::new(x, 0.6, z), rot, None);
    }
}
