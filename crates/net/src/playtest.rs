//! Local opt-in input driver for repeatable mixed-renderer playtests.
//! All edits still use the game's crosshair, reach and server validation.
use primal_voxel::Pos;
use std::{
    io::{self, BufRead},
    sync::mpsc,
};
pub enum Action {
    Look(f32, f32),
    Click(u8),
    Fly,
    Pose,
    Plant(Pos),
    Photo,
    Creatures,
    CreatureTrace(bool),
    Reconnect,
    Quit,
}
pub struct Playtest {
    rx: mpsc::Receiver<String>,
    watch: Option<Pos>,
    last: Option<(u64, u8)>,
}
impl Playtest {
    pub fn open() -> Self {
        let (tx, rx) = mpsc::sync_channel(8);
        std::thread::spawn(move || {
            for line in io::stdin().lock().lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            rx,
            watch: None,
            last: None,
        }
    }
    pub fn next_action(&mut self) -> Option<Action> {
        let line = self.rx.try_recv().ok()?;
        let fields: Vec<_> = line.split_whitespace().collect();
        match fields.as_slice() {
            ["look", a, b] => Some(Action::Look(a.parse().ok()?, b.parse().ok()?)),
            ["click", v] => {
                let block = v.parse().ok()?;
                (block <= 7).then_some(Action::Click(block))
            }
            ["watch", x, y, z] => {
                let p = [x.parse().ok()?, y.parse().ok()?, z.parse().ok()?];
                self.watch = Some(p);
                self.last = None;
                None
            }
            ["fly"] => Some(Action::Fly),
            ["plant", x, y, z] => Some(Action::Plant([
                x.parse().ok()?,
                y.parse().ok()?,
                z.parse().ok()?,
            ])),
            ["pose"] => Some(Action::Pose),
            ["photo"] => Some(Action::Photo),
            ["creature-trace", "on"] => Some(Action::CreatureTrace(true)),
            ["creature-trace", "off"] => Some(Action::CreatureTrace(false)),
            ["creatures"] => Some(Action::Creatures),
            ["reconnect"] => Some(Action::Reconnect),
            ["quit"] => Some(Action::Quit),
            _ => None,
        }
    }
    pub fn watch(&self) -> Option<Pos> {
        self.watch
    }
    pub fn observe(&mut self, replica: &crate::Replica, id: u64, peers: usize, mesh_ready: bool) {
        let Some(pos) = self.watch else { return };
        let c = primal_voxel::chunk_at(pos);
        if mesh_ready
            && let (Some(rev), Some(value)) = (replica.revision(c), replica.world.lookup(pos))
            && self.last != Some((rev, value))
        {
            self.last = Some((rev, value));
            println!(
                "OBSERVED {id} {peers} {rev} {value} {} {} {}",
                pos[0], pos[1], pos[2]
            );
        }
    }
}

/// Report the last authoritative pose received by an opt-in test client.
pub fn report_pose(actor: &crate::Actor) {
    let [x, y, z] = actor.position;
    println!("POSE {} {} {x} {y} {z}", actor.id, actor.flying);
}

/// Stable rolling digest of the sequence of per-frame visible geometry counts.
pub fn geometry_digest(previous: u64, chunks: usize, quads: usize) -> u64 {
    [chunks, quads].into_iter().fold(previous, |h, n| {
        (h ^ n as u64).wrapping_mul(1_099_511_628_211)
    })
}
