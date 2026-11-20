use primal_voxel::{Pos, World};
use std::{
    io,
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

struct Snapshot {
    frame: usize,
    seed: u64,
    edits: Vec<(Pos, u8)>,
    queued: Instant,
}

pub struct Ack {
    pub frame: usize,
    pub wait: Duration,
    pub disk: Duration,
    pub result: io::Result<()>,
}

/// One writer, one in-flight snapshot. New requests coalesce at the latest world revision.
pub struct Autosave {
    sender: Option<mpsc::Sender<Snapshot>>,
    replies: mpsc::Receiver<Ack>,
    worker: Option<thread::JoinHandle<()>>,
    busy: bool,
    pending: Option<usize>,
}

impl Autosave {
    pub fn new(path: PathBuf) -> io::Result<Self> {
        let (sender, requests) = mpsc::channel::<Snapshot>();
        let (done, replies) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("primal-save".into())
            .spawn(move || {
                for snapshot in requests {
                    let start = Instant::now();
                    let result =
                        primal_storage::save_snapshot(&path, snapshot.seed, &snapshot.edits);
                    let ack = Ack {
                        frame: snapshot.frame,
                        wait: start.duration_since(snapshot.queued),
                        disk: start.elapsed(),
                        result,
                    };
                    if done.send(ack).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            replies,
            worker: Some(worker),
            busy: false,
            pending: None,
        })
    }
    pub fn request(&mut self, world: &World, frame: usize) -> io::Result<()> {
        self.pending = Some(frame);
        self.dispatch(world)
    }
    fn dispatch(&mut self, world: &World) -> io::Result<()> {
        if !self.busy
            && let Some(frame) = self.pending.take()
        {
            let snapshot = Snapshot {
                frame,
                seed: world.seed(),
                edits: world.edits(),
                queued: Instant::now(),
            };
            self.sender
                .as_ref()
                .unwrap()
                .send(snapshot)
                .map_err(io::Error::other)?;
            self.busy = true;
        }
        Ok(())
    }
    pub fn poll(&mut self, world: &World) -> io::Result<Option<Ack>> {
        match self.replies.try_recv() {
            Ok(ack) => {
                self.busy = false;
                self.dispatch(world)?;
                Ok(Some(ack))
            }
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(e) => Err(io::Error::other(e)),
        }
    }
    pub fn flush(&mut self, world: &World) -> io::Result<()> {
        let mut last = Ok(());
        while self.busy {
            last = self.replies.recv().map_err(io::Error::other)?.result;
            self.busy = false;
            self.dispatch(world)?;
        }
        last
    }
}

impl Drop for Autosave {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn in_flight_snapshot_then_two_edits_commits_latest_state() {
        let folder = std::env::temp_dir().join(format!("primal-save-test-{}", std::process::id()));
        let path = folder.join("world.pvx");
        let mut writer = Autosave::new(path.clone()).unwrap();
        let mut world = World::new(9);
        writer.request(&world, 0).unwrap();
        world.set([0, 0, 0], 0);
        writer.request(&world, 1).unwrap();
        world.set([1, 0, 0], 0);
        writer.request(&world, 2).unwrap();
        writer.flush(&world).unwrap();
        assert_eq!(
            world.edits(),
            primal_storage::load_or_create(&path, 4).unwrap().edits()
        );
        drop(writer);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(folder).unwrap();
    }
}
