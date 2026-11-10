//! Headless test clients: connect N bots that fly around with their jetpacks.
//!
//! Usage: bot [--addr HOST:PORT] [--count N] [--seconds S]

use renet::{DefaultChannel, RenetClient};
use renet_netcode::{ClientAuthentication, NetcodeClientTransport};
use shared::protocol::{ClientMsg, PlayerInput, ServerMsg};
use shared::{DQuat, Vec3, decode, encode};
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant, SystemTime};

struct Bot {
    client: RenetClient,
    transport: NetcodeClientTransport,
    seq: u32,
    snapshots: u32,
    grids: u32,
    hello: bool,
}

fn main() {
    let mut addr: SocketAddr = format!("127.0.0.1:{}", shared::DEFAULT_PORT).parse().unwrap();
    let mut count = 4;
    let mut seconds = 30.0;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let v = args.next().expect("value");
        match a.as_str() {
            "--addr" => addr = v.parse().expect("addr"),
            "--count" => count = v.parse().expect("count"),
            "--seconds" => seconds = v.parse().expect("seconds"),
            other => panic!("unknown argument {other}"),
        }
    }
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    let mut bots: Vec<Bot> = (0..count)
        .map(|i| {
            let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
            let auth = ClientAuthentication::Unsecure {
                protocol_id: shared::PROTOCOL_ID,
                client_id: now.as_nanos() as u64 ^ (i as u64 * 7919 + 1),
                server_addr: addr,
                user_data: None,
            };
            Bot {
                client: RenetClient::new(server::client_connection_config()),
                transport: NetcodeClientTransport::new(now, auth, socket).unwrap(),
                seq: 0,
                snapshots: 0,
                grids: 0,
                hello: false,
            }
        })
        .collect();
    let start = Instant::now();
    let dt = Duration::from_millis(16);
    let mut last_stats = None;
    while start.elapsed().as_secs_f64() < seconds {
        let t = start.elapsed().as_secs_f32();
        for (i, b) in bots.iter_mut().enumerate() {
            b.client.update(dt);
            let _ = b.transport.update(dt, &mut b.client);
            if b.client.is_connected() {
                if !b.hello {
                    b.hello = true;
                    b.client.send_message(DefaultChannel::ReliableOrdered, encode(&ClientMsg::Hello { name: format!("bot{i}") }));
                }
                for ch in [1u8, 0u8] {
                    while let Some(m) = b.client.receive_message(ch) {
                        match decode::<ServerMsg>(&m) {
                            Some(ServerMsg::Snapshot(_)) => b.snapshots += 1,
                            Some(ServerMsg::FullGrid(_)) => b.grids += 1,
                            Some(ServerMsg::Stats(s)) => last_stats = Some(s),
                            _ => {}
                        }
                    }
                }
                b.seq += 1;
                let phase = t * 0.5 + i as f32;
                let input = PlayerInput {
                    seq: b.seq,
                    movement: Vec3::new(phase.sin(), (phase * 0.7).cos() * 0.5, phase.cos()),
                    rotation: Vec3::ZERO,
                    look: DQuat::from_rotation_y(phase as f64 * 0.3),
                    dampeners: true,
                    jetpack: true,
                    cruise: false,
                };
                b.client.send_message(DefaultChannel::Unreliable, encode(&ClientMsg::Input(input)));
            }
            let _ = b.transport.send_packets(&mut b.client);
        }
        std::thread::sleep(dt);
    }
    for (i, b) in bots.iter_mut().enumerate() {
        println!(
            "bot{i}: connected={} snapshots={} grids={} rtt={:.0}ms",
            b.client.is_connected(),
            b.snapshots,
            b.grids,
            b.client.rtt() * 1000.0
        );
        b.transport.disconnect();
    }
    if let Some(s) = last_stats {
        println!(
            "server: tick {:.2} ms (scripts {:.2} ms, physics {:.2} ms), {} players, {} grids, {} blocks, {} scripts",
            s.tick_ms, s.script_ms, s.physics_ms, s.players, s.grids, s.blocks, s.scripts
        );
    }
}
