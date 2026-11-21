use primal_storage::RegionStore;
use primal_streaming::{StreamConfig, Streamer};
use primal_voxel::World;
use serde::Deserialize;
use serde_json::json;
use std::{
    env, fs, io,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Config {
    world: PathBuf,
    bind: SocketAddr,
    seed: u64,
    tick_hz: u32,
    max_players: usize,
    ticks: Option<u64>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            world: "server-world.pvx".into(),
            bind: "127.0.0.1:47620".parse().unwrap(),
            seed: 20_261_120,
            tick_hz: 60,
            max_players: 8,
            ticks: None,
        }
    }
}
fn config() -> Result<Config, Box<dyn std::error::Error>> {
    let mut c = Config::default();
    let mut args = env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("Valeur absente pour {flag}"))?;
        match flag.as_str() {
            "--config" => c = serde_json::from_slice(&fs::read(value)?)?,
            "--world" => c.world = value.into(),
            "--bind" => c.bind = value.parse()?,
            "--ticks" => c.ticks = Some(value.parse()?),
            "--max-players" => c.max_players = value.parse()?,
            "--tick-hz" => c.tick_hz = value.parse()?,
            _ => return Err(format!("Option inconnue {flag}").into()),
        }
    }
    if !(10..=120).contains(&c.tick_hz) || !(1..=32).contains(&c.max_players) {
        return Err("tick_hz doit être entre 10 et 120 et max_players entre 1 et 32".into());
    }
    Ok(c)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let c = config()?;
    let store = RegionStore::open(&c.world, c.seed)?;
    let mut stream = Streamer::headless(
        World::streamed(store.seed()),
        store,
        StreamConfig::default(),
    )?;
    let running = Arc::new(AtomicBool::new(true));
    let signal = running.clone();
    ctrlc::set_handler(move || signal.store(false, Ordering::Release))?;
    let step = Duration::from_secs_f64(1. / f64::from(c.tick_hz));
    let mut next = Instant::now();
    let mut tick = 0u64;
    println!(
        "{}",
        json!({"event":"started","tick_hz":c.tick_hz,"world":c.world})
    );
    while running.load(Ordering::Acquire) && c.ticks.is_none_or(|limit| tick < limit) {
        let frame = stream.tick_views(&[]);
        if let Some(error) = frame.errors.first() {
            return Err(io::Error::other(error.clone()).into());
        }
        tick += 1;
        next += step;
        thread::sleep(next.saturating_duration_since(Instant::now()));
        if next.elapsed() > step * 4 {
            next = Instant::now();
        }
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    while !stream.saves_idle() {
        let f = stream.tick_views(&[]);
        if !f.errors.is_empty() || Instant::now() >= deadline {
            return Err("Impossible de confirmer la sauvegarde".into());
        }
        thread::sleep(Duration::from_millis(1));
    }
    println!(
        "{}",
        json!({"event":"stopped","ticks":tick,"resident":stream.world().resident_len()})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{}", json!({"event":"error","message":e.to_string()}));
        std::process::exit(1);
    }
}
