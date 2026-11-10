//! Headless dedicated server.
//!
//! Usage: server [--port N] [--save FILE | --no-save] [--latency MS] [--loss PCT] [--stress]

use server::{GameServer, ServerConfig};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let mut cfg = ServerConfig::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().unwrap_or_else(|| panic!("{a} needs a value"));
        match a.as_str() {
            "--port" => cfg.bind.set_port(val().parse().expect("port")),
            "--save" => cfg.save_path = Some(val().into()),
            "--no-save" => cfg.save_path = None,
            "--latency" => cfg.sim_latency = Duration::from_millis(val().parse().expect("ms")),
            "--loss" => cfg.sim_loss = val().parse().expect("percent"),
            "--stress" => cfg.stress = true,
            "-h" | "--help" => {
                println!("server [--port N] [--save FILE | --no-save] [--latency MS] [--loss PCT] [--stress]");
                return;
            }
            other => panic!("unknown argument {other}"),
        }
    }
    let mut server = GameServer::new(cfg).expect("could not start server");
    log::info!("listening on {}", server.local_addr());
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    ctrlc::set_handler(move || s.store(true, Ordering::Relaxed)).expect("signal handler");
    server.run(stop);
}
