mod disk;
mod metrics;
mod room;
use primal_storage::RegionStore;
use primal_streaming::{StreamConfig, Streamer};
use primal_voxel::World;
use serde::Deserialize;
use serde_json::json;
use std::{
    env, fs,
    io::{self, BufRead},
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
    let (disk, revisions, profiles) = disk::Disk::open(&c.world, store.clone())?;
    let mut stream = Streamer::headless(
        World::streamed(store.seed()),
        store,
        StreamConfig::default(),
    )?;
    let net = primal_net::ServerTransport::bind(c.bind, c.max_players)?;
    let address = net.local_addr();
    let mut room = room::Room::new(net, disk, revisions, profiles);
    let running = Arc::new(AtomicBool::new(true));
    let signal = running.clone();
    ctrlc::set_handler(move || signal.store(false, Ordering::Release))?;
    let (commands, admin) = std::sync::mpsc::sync_channel(32);
    thread::spawn(move || {
        for line in io::stdin().lock().lines().map_while(Result::ok) {
            if commands.send(line).is_err() {
                break;
            }
        }
    });
    let step = Duration::from_secs_f64(1. / f64::from(c.tick_hz));
    let mut next = Instant::now();
    let mut metrics = metrics::Metrics::new();
    let mut tick = 0u64;
    println!(
        "{}",
        json!({"event":"started","address":address,"tick_hz":c.tick_hz,"world":c.world})
    );
    while running.load(Ordering::Acquire) && c.ticks.is_none_or(|limit| tick < limit) {
        for cmd in admin.try_iter().take(16) {
            let args: Vec<_> = cmd.split_whitespace().collect();
            match args.as_slice() {
                ["stop"] => running.store(false, Ordering::Release),
                ["teleport", id, x, y, z] => {
                    let ok = match (id.parse(), x.parse(), y.parse(), z.parse()) {
                        (Ok(id), Ok(x), Ok(y), Ok(z)) => room.teleport(id, [x, y, z]),
                        _ => false,
                    };
                    println!("{}", json!({"event":"teleport","ok":ok}));
                }
                _ => eprintln!(
                    "{}",
                    json!({"event":"error","message":"Commande console inconnue"})
                ),
            }
        }
        let began = Instant::now();
        let streaming = room.tick(&mut stream, tick, step)?;
        metrics.record(began.elapsed(), streaming);
        tick += 1;
        next += step;
        thread::sleep(next.saturating_duration_since(Instant::now()));
        if metrics.len() >= 120 {
            metrics.report(
                tick,
                stream.world().resident_len(),
                room.players(),
                room.traffic(),
            );
        }
        if next.elapsed() > step * 4 {
            next = Instant::now();
        }
    }
    metrics.report(
        tick,
        stream.world().resident_len(),
        room.players(),
        room.traffic(),
    );
    room.finish(stream.world_mut())?;
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
