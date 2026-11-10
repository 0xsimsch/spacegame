//! Network connection to a server, plus the optional embedded ("host & play") server.

use renet::{DefaultChannel, RenetClient};
use renet_netcode::{ClientAuthentication, NetcodeClientTransport};
use shared::protocol::{ClientMsg, ServerMsg};
use shared::{decode, encode};
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

pub struct Net {
    client: RenetClient,
    transport: NetcodeClientTransport,
    name: String,
    hello_sent: bool,
}

impl Net {
    pub fn connect(addr: &str, name: &str) -> Result<Self, String> {
        let server: SocketAddr = addr
            .to_socket_addrs()
            .map_err(|e| format!("bad address: {e}"))?
            .find(|a| a.is_ipv4())
            .ok_or("could not resolve address")?;
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
        let auth = ClientAuthentication::Unsecure {
            protocol_id: shared::PROTOCOL_ID,
            client_id: now.as_nanos() as u64,
            server_addr: server,
            user_data: None,
        };
        let transport = NetcodeClientTransport::new(now, auth, socket).map_err(|e| e.to_string())?;
        Ok(Self {
            client: RenetClient::new(server::client_connection_config()),
            transport,
            name: name.to_string(),
            hello_sent: false,
        })
    }

    /// Pump the connection; returns received messages.
    pub fn update(&mut self, dt: Duration) -> Result<Vec<ServerMsg>, String> {
        self.client.update(dt);
        self.transport.update(dt, &mut self.client).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        if self.client.is_connected() {
            if !self.hello_sent {
                self.hello_sent = true;
                self.send(&ClientMsg::Hello { name: self.name.clone() });
            }
            for ch in [1u8, 0u8] {
                while let Some(bytes) = self.client.receive_message(ch) {
                    if let Some(m) = decode::<ServerMsg>(&bytes) {
                        out.push(m);
                    }
                }
            }
        }
        Ok(out)
    }

    pub fn send(&mut self, msg: &ClientMsg) {
        if !self.client.is_connected() {
            return;
        }
        let ch = match msg {
            ClientMsg::Input(_) => DefaultChannel::Unreliable,
            _ => DefaultChannel::ReliableOrdered,
        };
        self.client.send_message(ch, encode(msg));
    }

    pub fn flush(&mut self) {
        let _ = self.transport.send_packets(&mut self.client);
    }

    pub fn is_connected(&self) -> bool {
        self.client.is_connected()
    }

    pub fn disconnect_reason(&self) -> Option<String> {
        if let Some(r) = self.transport.disconnect_reason() {
            return Some(r.to_string());
        }
        self.client.disconnect_reason().map(|r| r.to_string())
    }

    pub fn disconnect(&mut self) {
        self.transport.disconnect();
    }

    pub fn rtt_ms(&self) -> f64 {
        self.client.rtt() * 1000.0
    }

    pub fn network_info(&self) -> renet::NetworkInfo {
        self.client.network_info()
    }
}

/// A server running on a background thread of the client process.
pub struct Embedded {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pub port: u16,
}

impl Embedded {
    pub fn start(port: u16, save_path: PathBuf) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();
        let s = stop.clone();
        let thread = std::thread::Builder::new()
            .name("embedded-server".into())
            .spawn(move || {
                let cfg = server::ServerConfig {
                    bind: SocketAddr::from(([0, 0, 0, 0], port)),
                    save_path: Some(save_path),
                    ..Default::default()
                };
                match server::GameServer::new(cfg) {
                    Ok(mut srv) => {
                        let _ = tx.send(Ok(srv.local_addr().port()));
                        srv.run(s);
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e.to_string()));
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let port = rx
            .recv_timeout(Duration::from_secs(20))
            .map_err(|_| "server did not start".to_string())??;
        Ok(Self { stop, thread: Some(thread), port })
    }
}

impl Drop for Embedded {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
