use primal_streaming::{ChunkStore, Frame, MeshKind, StreamConfig, Streamer, View};
use primal_voxel::{ChunkPos, Pos, World};
use std::collections::BTreeMap;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Gate {
    state: Mutex<(bool, bool)>,
    changed: Condvar,
}

impl Gate {
    fn stop_first(&self) {
        let mut state = self.state.lock().unwrap();
        if !state.0 {
            state.0 = true;
            self.changed.notify_all();
            self.changed.wait_while(state, |s| !s.1).unwrap().1 = true;
        }
    }

    fn entered(&self) -> bool {
        self.state.lock().unwrap().0
    }

    fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.changed.notify_all();
    }
}

type ChunkEdits = Vec<(Pos, u8)>;

#[derive(Default)]
struct Store {
    chunks: Mutex<BTreeMap<ChunkPos, ChunkEdits>>,
    loads: AtomicUsize,
    fail_save: AtomicBool,
    load_gate: Option<Arc<Gate>>,
    save_gate: Option<Arc<Gate>>,
}

impl ChunkStore for Store {
    fn seed(&self) -> u64 {
        42
    }

    fn load_chunk(&self, chunk: ChunkPos) -> io::Result<ChunkEdits> {
        self.loads.fetch_add(1, Ordering::SeqCst);
        if let Some(gate) = &self.load_gate {
            gate.stop_first();
        }
        Ok(self
            .chunks
            .lock()
            .unwrap()
            .get(&chunk)
            .cloned()
            .unwrap_or_default())
    }

    fn save_chunk(&self, chunk: ChunkPos, edits: &[(Pos, u8)]) -> io::Result<()> {
        if let Some(gate) = &self.save_gate {
            gate.stop_first();
        }
        if self.fail_save.load(Ordering::SeqCst) {
            return Err(io::Error::other("disk full"));
        }
        self.chunks.lock().unwrap().insert(chunk, edits.to_owned());
        Ok(())
    }
}

fn config() -> StreamConfig {
    StreamConfig {
        load_radius: 0,
        render_radius: 0,
        simulation_radius: 0,
        preload_radius: 0,
        max_chunks: 1,
        workers: 3,
        max_jobs: 4,
        ..Default::default()
    }
}

fn view(x: i32) -> View {
    View {
        block: [x, 40, 0],
        forward: [1.0, 0.0, 0.0],
        velocity: [0.0; 3],
    }
}

fn until<P: Send + 'static>(
    stream: &mut Streamer<P>,
    focus: View,
    mut ready: impl FnMut(&Streamer<P>, &Frame<P>) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let frame = stream.tick(focus);
        if ready(stream, &frame) {
            break;
        }
        assert!(Instant::now() < deadline, "timeout {:?}", stream.stats());
        std::thread::sleep(Duration::from_millis(1));
    }
}

const CELL: Pos = [0, 40, 0];
const ORIGIN: ChunkPos = [0, 2, 0];

#[test]
fn edit_survives_old_load_across_teleport_and_return() {
    let gate = Arc::new(Gate::default());
    let disk = Arc::new(Store {
        load_gate: Some(gate.clone()),
        ..Default::default()
    });
    let mut stream = Streamer::new(
        World::streamed(42),
        disk.clone(),
        config(),
        MeshKind::Greedy,
        |_| ((), 0),
    )
    .unwrap();
    until(&mut stream, view(0), |_, _| gate.entered());
    for _ in 0..10 {
        stream.tick(view(0));
    }
    assert_eq!(disk.loads.load(Ordering::SeqCst), 1);
    until(&mut stream, view(1_000_000), |s, _| {
        s.world().is_resident([62_500, 2, 0])
    });
    until(&mut stream, view(0), |s, _| s.world().is_resident(ORIGIN));
    assert!(stream.world_mut().set(CELL, 7));
    gate.release();
    until(&mut stream, view(0), |s, _| {
        s.stats().stale_loads > 0 && s.saves_idle()
    });
    assert_eq!(stream.world().get(CELL), 7);
    assert_eq!(disk.chunks.lock().unwrap()[&ORIGIN], vec![(CELL, 7)]);
}

#[test]
fn edit_during_mesh_only_publishes_the_latest_revision() {
    let gate = Arc::new(Gate::default());
    let capture = gate.clone();
    let mut stream = Streamer::new(
        World::streamed(42),
        Arc::new(Store::default()),
        config(),
        MeshKind::Faces,
        move |faces| {
            capture.stop_first();
            (faces.len(), 0)
        },
    )
    .unwrap();
    until(&mut stream, view(0), |_, _| gate.entered());
    let before = stream.world().mesh_revision(ORIGIN).unwrap();
    assert!(stream.world_mut().set(CELL, 7));
    let after = stream.world().mesh_revision(ORIGIN).unwrap();
    assert_ne!(before, after);
    gate.release();
    let mut visible = false;
    until(&mut stream, view(0), |s, frame| {
        for mesh in &frame.uploads {
            assert_eq!(mesh.revision, after);
            visible = true;
        }
        visible && s.stats().stale_meshes > 0
    });
}

#[test]
fn late_save_ack_cannot_evict_a_newer_dirty_edit() {
    let gate = Arc::new(Gate::default());
    let disk = Arc::new(Store {
        save_gate: Some(gate.clone()),
        ..Default::default()
    });
    let mut stream = Streamer::new(
        World::streamed(42),
        disk.clone(),
        config(),
        MeshKind::Greedy,
        |_| ((), 0),
    )
    .unwrap();
    until(&mut stream, view(0), |s, _| s.world().is_resident(ORIGIN));
    assert!(stream.world_mut().set(CELL, 6));
    until(&mut stream, view(0), |_, _| gate.entered());
    assert!(stream.world_mut().set(CELL, 7));
    stream.tick(view(1_024));
    assert!(stream.world().is_resident(ORIGIN));
    gate.release();
    until(&mut stream, view(1_024), |s, _| {
        !s.world().is_resident(ORIGIN)
    });
    assert_eq!(disk.chunks.lock().unwrap()[&ORIGIN], vec![(CELL, 7)]);
    until(&mut stream, view(0), |s, _| s.world().is_resident(ORIGIN));
    assert_eq!(stream.world().get(CELL), 7);
}

#[test]
fn failed_background_save_pins_chunk_and_retries() {
    let disk = Arc::new(Store::default());
    disk.fail_save.store(true, Ordering::SeqCst);
    let mut stream = Streamer::new(
        World::streamed(42),
        disk.clone(),
        config(),
        MeshKind::Greedy,
        |_| ((), 0),
    )
    .unwrap();
    until(&mut stream, view(0), |s, _| s.world().is_resident(ORIGIN));
    assert!(stream.world_mut().set(CELL, 7));
    until(&mut stream, view(-512), |s, frame| {
        s.has_save_error() && !frame.errors.is_empty()
    });
    assert_eq!(stream.world().get(CELL), 7);
    assert!(!stream.saves_idle());
    disk.fail_save.store(false, Ordering::SeqCst);
    stream.retry_errors();
    until(&mut stream, view(-512), |s, _| {
        s.saves_idle() && !s.world().is_resident(ORIGIN)
    });
    until(&mut stream, view(0), |s, _| s.world().is_resident(ORIGIN));
    assert_eq!(stream.world().get(CELL), 7);
}
