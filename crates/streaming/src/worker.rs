use crate::{ChunkStore, MeshKind, Stage, Timing};
use primal_voxel::{ChunkPos, Face, GeneratedChunk, MeshSnapshot, Pos, generate_for};
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

type PrepareFaces<P> = dyn Fn(&[Face]) -> (P, usize) + Send + Sync;
type PrepareSnapshot<P> = dyn Fn(&MeshSnapshot) -> (P, usize) + Send + Sync;

pub(crate) enum Prepare<P> {
    Faces(MeshKind, Box<PrepareFaces<P>>),
    Snapshot(Box<PrepareSnapshot<P>>),
}

pub(crate) enum Work {
    Load,
    Mesh(MeshSnapshot),
    Save(Vec<(Pos, u8)>),
}

pub(crate) enum Product<P> {
    Load(GeneratedChunk, Vec<(Pos, u8)>),
    Mesh(P, usize),
    Save,
}

pub(crate) struct Job {
    pub(crate) id: u64,
    pub(crate) chunk: ChunkPos,
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) queued: Instant,
    pub(crate) work: Work,
}

pub(crate) struct Done<P> {
    pub(crate) id: u64,
    pub(crate) result: io::Result<Option<Product<P>>>,
    pub(crate) timings: Vec<Timing>,
    pub(crate) finished: Instant,
    pub(crate) cpu: Option<Duration>,
}

impl Job {
    fn run<P>(
        &self,
        store: &dyn ChunkStore,
        prepare: &Prepare<P>,
        timings: &mut Vec<Timing>,
    ) -> io::Result<Option<Product<P>>> {
        let mut record = |stage, started: Instant| {
            timings.push(Timing {
                chunk: self.chunk,
                stage,
                duration: started.elapsed(),
            })
        };
        let queue = match self.work {
            Work::Load => Stage::QueueLoad,
            Work::Mesh(_) => Stage::QueueMesh,
            Work::Save(_) => Stage::QueueSave,
        };
        record(queue, self.queued);
        let cancelled = || self.stop.load(Ordering::Acquire);
        if cancelled() {
            return Ok(None);
        }
        let product = match &self.work {
            Work::Load => {
                let started = Instant::now();
                let edits = store.load_chunk(self.chunk);
                record(Stage::Load, started);
                let edits = edits?;
                if cancelled() {
                    return Ok(None);
                }
                let started = Instant::now();
                let data = generate_for(store.preset(), store.seed(), self.chunk)
                    .ok_or_else(|| io::Error::other("chunk outside generator bounds"))?;
                record(Stage::Generate, started);
                Product::Load(data, edits)
            }
            Work::Mesh(snapshot) => match prepare {
                Prepare::Faces(kind, prepare) => {
                    let started = Instant::now();
                    let faces = match kind {
                        MeshKind::GreedyLocal => snapshot.greedy_mesh_local(),
                        MeshKind::Greedy => snapshot.greedy_mesh(),
                        MeshKind::Binary => snapshot.binary_greedy_mesh(),
                        MeshKind::Faces => snapshot.mesh(),
                    };
                    record(Stage::Mesh, started);
                    if cancelled() {
                        return Ok(None);
                    }
                    let started = Instant::now();
                    let (payload, bytes) = prepare(&faces);
                    record(Stage::Prepare, started);
                    Product::Mesh(payload, bytes)
                }
                Prepare::Snapshot(prepare) => {
                    let started = Instant::now();
                    let (payload, bytes) = prepare(snapshot);
                    record(Stage::Mesh, started);
                    Product::Mesh(payload, bytes)
                }
            },
            Work::Save(edits) => {
                let started = Instant::now();
                let saved = store.save_chunk(self.chunk, edits);
                record(Stage::Save, started);
                saved?;
                Product::Save
            }
        };
        Ok(Some(product))
    }
}

pub(crate) struct Pool<P> {
    // Only the owner submits jobs and retains one credit until each result is consumed.
    tx: Option<mpsc::Sender<Job>>,
    pub(crate) rx: mpsc::Receiver<Done<P>>,
    threads: Vec<JoinHandle<()>>,
    waiting: Arc<AtomicUsize>,
    peak_waiting: AtomicUsize,
}

impl<P: Send + 'static> Pool<P> {
    pub(crate) fn new(
        count: usize,
        store: Arc<dyn ChunkStore>,
        prepare: Arc<Prepare<P>>,
    ) -> io::Result<Self> {
        let (tx, tasks) = mpsc::channel::<Job>();
        let tasks = Arc::new(Mutex::new(tasks));
        let (out, rx) = mpsc::channel();
        let waiting = Arc::new(AtomicUsize::new(0));
        let mut pool = Self {
            waiting: waiting.clone(),
            peak_waiting: AtomicUsize::new(0),
            tx: Some(tx),
            rx,
            threads: Vec::new(),
        };
        for i in 0..count {
            let (tasks, out, store, prepare) =
                (tasks.clone(), out.clone(), store.clone(), prepare.clone());
            let waiting = waiting.clone();
            let thread = thread::Builder::new()
                .name(format!("voxel-{i}"))
                .spawn(move || {
                    loop {
                        let job = {
                            let queue = tasks.lock().expect("worker receiver poisoned");
                            queue.recv()
                        };
                        let Ok(job) = job else { break };
                        waiting.fetch_sub(1, Ordering::Relaxed);
                        let cpu = cpu_time::ThreadTime::try_now().ok();
                        let mut timings = Vec::with_capacity(3);
                        let result = catch_unwind(AssertUnwindSafe(|| {
                            job.run(store.as_ref(), prepare.as_ref(), &mut timings)
                        }))
                        .unwrap_or_else(|_| Err(io::Error::other("stream worker panicked")));
                        if out
                            .send(Done {
                                id: job.id,
                                result,
                                timings,
                                finished: Instant::now(),
                                cpu: cpu.and_then(|v| v.try_elapsed().ok()),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                })?;
            pool.threads.push(thread);
        }
        Ok(pool)
    }

    pub(crate) fn peak_waiting(&self) -> usize {
        self.peak_waiting.load(Ordering::Relaxed)
    }
    pub(crate) fn send(&self, job: Job) {
        self.peak_waiting.fetch_max(
            1 + self.waiting.fetch_add(1, Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.tx
            .as_ref()
            .expect("live pool")
            .send(job)
            .expect("stream workers exited");
    }
}

impl<P> Drop for Pool<P> {
    fn drop(&mut self) {
        self.tx.take();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}
