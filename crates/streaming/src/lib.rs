//! Bounded CPU jobs and versioned publication. The caller owns the graphics device.
mod plan;
mod worker;
use plan::Plan;
use primal_storage::RegionStore;
use primal_voxel::{ChunkPos, Face, LoadTicket, Pos, World};
use std::{
    collections::{HashMap, HashSet},
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use worker::{Done, Job, Pool, Product, Work};

pub trait ChunkStore: Send + Sync + 'static {
    fn seed(&self) -> u64;
    fn load_chunk(&self, c: ChunkPos) -> io::Result<Vec<(Pos, u8)>>;
    fn save_chunk(&self, c: ChunkPos, edits: &[(Pos, u8)]) -> io::Result<()>;
}
impl<T: ChunkStore> ChunkStore for Arc<T> {
    fn seed(&self) -> u64 {
        (**self).seed()
    }
    fn load_chunk(&self, c: ChunkPos) -> io::Result<Vec<(Pos, u8)>> {
        (**self).load_chunk(c)
    }
    fn save_chunk(&self, c: ChunkPos, e: &[(Pos, u8)]) -> io::Result<()> {
        (**self).save_chunk(c, e)
    }
}
impl ChunkStore for RegionStore {
    fn seed(&self) -> u64 {
        RegionStore::seed(self)
    }
    fn load_chunk(&self, c: ChunkPos) -> io::Result<Vec<(Pos, u8)>> {
        RegionStore::load_chunk(self, c)
    }
    fn save_chunk(&self, c: ChunkPos, e: &[(Pos, u8)]) -> io::Result<()> {
        RegionStore::save_chunk(self, c, e)
    }
}
#[derive(Clone, Copy)]
pub enum MeshKind {
    Binary,
    Greedy,
    Faces,
}
#[derive(Clone, Copy, Debug)]
pub enum Stage {
    Select,
    Dispatch,
    Cancel,
    Evict,
    Publish,
    QueueLoad,
    QueueMesh,
    QueueSave,
    Load,
    Generate,
    Mesh,
    Prepare,
    Save,
    Accept,
}
impl Stage {
    pub fn name(self) -> &'static str {
        match self {
            Self::Select => "plan_select",
            Self::Dispatch => "queue_dispatch",
            Self::Cancel => "cancel_scan",
            Self::Evict => "evict_save_scan",
            Self::Publish => "publish_cpu",
            Self::QueueLoad => "load_queue",
            Self::QueueMesh => "mesh_queue",
            Self::QueueSave => "save_queue",
            Self::Load => "region_read",
            Self::Generate => "generate",
            Self::Mesh => "remesh",
            Self::Prepare => "prepare_buffers",
            Self::Save => "save_disk",
            Self::Accept => "result_wait",
        }
    }
}
pub struct Timing {
    pub stage: Stage,
    pub chunk: ChunkPos,
    pub duration: Duration,
}
#[derive(Debug, Clone)]
pub struct StreamConfig {
    pub load_radius: i32,
    pub render_radius: i32,
    pub simulation_radius: i32,
    pub preload_radius: i32,
    pub max_chunks: usize,
    pub workers: usize,
    pub max_jobs: usize,
    pub uploads_per_tick: usize,
    pub resident_bytes: usize,
    pub prepared_bytes: usize,
    pub mesh_cache_bytes: usize,
    pub main_budget: Duration,
}
// Worst-case prepared vertices/indices for a 16³ checkerboard; includes spare container capacity.
// The client representation remains below this per-job allowance.
pub const MESH_RESERVATION: usize = 6 << 20;
impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            load_radius: 5,
            render_radius: 4,
            simulation_radius: 1,
            preload_radius: 2,
            max_chunks: 1536,
            workers: 4,
            max_jobs: 24,
            uploads_per_tick: 12,
            resident_bytes: 64 << 20,
            prepared_bytes: 96 << 20,
            mesh_cache_bytes: 160 << 20,
            main_budget: Duration::from_millis(4),
        }
    }
}
impl StreamConfig {
    pub fn chunk_limit(&self) -> usize {
        self.max_chunks.min(self.resident_bytes / (64 * 1024))
    }
    pub fn validate(&self) -> io::Result<()> {
        if !(0..=24).contains(&self.load_radius)
            || !(0..=self.load_radius).contains(&self.render_radius)
            || !(0..=self.load_radius).contains(&self.simulation_radius)
            || !(0..=24).contains(&self.preload_radius)
            || self.chunk_limit() == 0
            || !(1..=32).contains(&self.workers)
            || !(1..=256).contains(&self.max_jobs)
            || self.uploads_per_tick == 0
            || self.prepared_bytes < MESH_RESERVATION
            || self.mesh_cache_bytes == 0
            || self.main_budget.is_zero()
        {
            return Err(io::Error::other(
                "Rayons ou budgets de streaming incompatibles",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Copy)]
pub struct View {
    pub block: Pos,
    pub forward: [f32; 3],
    pub velocity: [f32; 3],
}
pub struct Upload<P> {
    pub chunk: ChunkPos,
    pub revision: u64,
    pub payload: P,
}
pub struct Frame<P> {
    pub uploads: Vec<Upload<P>>,
    pub evicted: Vec<ChunkPos>,
    pub errors: Vec<String>,
    pub timings: Vec<Timing>,
}
impl<P> Default for Frame<P> {
    fn default() -> Self {
        Self {
            uploads: Vec::new(),
            evicted: Vec::new(),
            errors: Vec::new(),
            timings: Vec::new(),
        }
    }
}
#[derive(Default, Debug)]
pub struct Stats {
    pub stale_loads: usize,
    pub stale_meshes: usize,
    pub peak_resident: usize,
    pub peak_jobs: usize,
    pub peak_mesh_bytes: usize,
    pub peak_waiting: usize,
    pub evictions: u64,
    pub cancelled_cpu_ns: u64,
    pub unavailable_cpu_samples: u64,
}
#[derive(Clone, Copy)]
enum Kind {
    Load(LoadTicket),
    Mesh(u64),
    Save(u64),
}
struct Running {
    chunk: ChunkPos,
    kind: Kind,
    stop: Arc<AtomicBool>,
}
pub struct Streamer<P> {
    world: World,
    config: StreamConfig,
    plan: Plan,
    pool: Pool<P>,
    jobs: HashMap<u64, Running>,
    next: u64,
    meshes: HashMap<ChunkPos, (u64, usize)>,
    mesh_bytes: usize,
    deferred: HashMap<ChunkPos, u64>,
    errors: HashSet<(ChunkPos, u8)>,
    stats: Stats,
    fairness_cursor: usize,
    ticks: u64,
    ready_mesh: Vec<Done<P>>,
    build_meshes: bool,
}
impl<P: Send + 'static> Streamer<P> {
    pub fn new(
        world: World,
        store: impl ChunkStore,
        config: StreamConfig,
        kind: MeshKind,
        prepare: impl Fn(&[Face]) -> (P, usize) + Send + Sync + 'static,
    ) -> io::Result<Self> {
        config.validate()?;
        if store.seed() != world.seed() {
            return Err(io::Error::other("Seed incompatible"));
        }
        let pool = Pool::new(config.workers, Arc::new(store), kind, Arc::new(prepare))?;
        Ok(Self {
            world,
            config,
            plan: Plan::default(),
            pool,
            jobs: HashMap::new(),
            next: 0,
            meshes: HashMap::new(),
            mesh_bytes: 0,
            deferred: HashMap::new(),
            errors: HashSet::new(),
            stats: Stats::default(),
            fairness_cursor: 0,
            ticks: 0,
            ready_mesh: Vec::new(),
            build_meshes: true,
        })
    }
    pub fn world(&self) -> &World {
        &self.world
    }
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }
    pub fn set_render_radius(&mut self, radius: i32) {
        let radius = radius.clamp(1, 24);
        if radius != self.config.render_radius {
            self.config.render_radius = radius;
            self.config.load_radius = self.config.load_radius.max(radius);
            self.plan = Plan::default();
        }
    }
    pub fn stats(&self) -> &Stats {
        &self.stats
    }
    pub fn saves_idle(&self) -> bool {
        !self
            .world
            .chunks()
            .iter()
            .any(|&c| self.world.chunk_dirty(c))
    }
    pub fn has_save_error(&self) -> bool {
        self.errors.iter().any(|&(_, k)| k == 2)
    }
    pub fn retry_errors(&mut self) {
        self.errors.clear()
    }
    pub fn visible_revision(&self, c: ChunkPos) -> Option<u64> {
        self.meshes.get(&c).map(|v| v.0)
    }
    fn halo_resident(&self, c: ChunkPos) -> bool {
        for axis in 0..3 {
            for step in [-1, 1] {
                let mut n = c;
                n[axis] += step;
                if self.plan.wants(n)
                    && !self.world.is_resident(n)
                    && !self.errors.contains(&(n, 0))
                {
                    return false;
                }
            }
        }
        true
    }
    fn active(&self, c: ChunkPos, k: u8) -> bool {
        self.jobs
            .values()
            .any(|j| j.chunk == c && !j.stop.load(Ordering::Relaxed) && j.kind.code() == k)
    }
    fn dispatch(&mut self, c: ChunkPos, kind: Kind, work: Work) {
        self.next += 1;
        let stop = Arc::new(AtomicBool::new(false));
        self.jobs.insert(
            self.next,
            Running {
                chunk: c,
                kind,
                stop: stop.clone(),
            },
        );
        self.pool.send(Job {
            id: self.next,
            chunk: c,
            work,
            stop,
            queued: Instant::now(),
        });
    }
    fn discard_mesh(&mut self, c: ChunkPos, out: &mut Frame<P>) {
        if let Some((_, b)) = self.meshes.remove(&c) {
            self.mesh_bytes -= b;
            if b > 0 {
                self.deferred.clear();
            }
            out.evicted.push(c)
        }
    }
    fn accept(&mut self, done: Done<P>, out: &mut Frame<P>) {
        let Some(job) = self.jobs.remove(&done.id) else {
            return;
        };
        let c = job.chunk;
        out.timings.extend(done.timings);
        out.timings.push(Timing {
            chunk: c,
            stage: Stage::Accept,
            duration: done.finished.elapsed(),
        });
        let stale = job.stop.load(Ordering::Acquire)
            || match job.kind {
                Kind::Mesh(rev) => {
                    self.world.mesh_revision(c) != Some(rev) || self.plan.render_rank(c).is_none()
                }
                Kind::Load(_) => !self.plan.wants(c),
                Kind::Save(_) => false,
            };
        if stale {
            if let Some(cpu) = done.cpu {
                self.stats.cancelled_cpu_ns += cpu.as_nanos() as u64;
            } else {
                self.stats.unavailable_cpu_samples += 1;
            }
            match job.kind {
                Kind::Load(t) => {
                    self.world.cancel_load(t);
                    self.stats.stale_loads += 1;
                }
                Kind::Mesh(_) => self.stats.stale_meshes += 1,
                _ => {}
            }
            return;
        }
        let product = match done.result {
            Ok(Some(p)) => p,
            Ok(None) => return,
            Err(e) => {
                if let Kind::Load(t) = job.kind {
                    self.world.cancel_load(t);
                }
                self.errors.insert((c, job.kind.code()));
                out.errors.push(format!("{c:?} {e}"));
                return;
            }
        };
        match (job.kind, product) {
            (Kind::Load(ticket), Product::Load(data, edits)) => {
                if !self.world.install_chunk(ticket, data, &edits) {
                    self.errors.insert((c, 0));
                    out.errors.push(format!("Données de chunk invalides {c:?}"));
                }
            }
            (Kind::Save(rev), Product::Save) => {
                self.world.mark_saved(c, rev);
            }
            (Kind::Mesh(rev), Product::Mesh(payload, bytes)) => {
                let old = self.meshes.get(&c).map_or(0, |p| p.1);
                if bytes > MESH_RESERVATION || bytes > self.config.mesh_cache_bytes {
                    self.errors.insert((c, 1));
                    out.errors.push(format!("Maillage hors budget {c:?}"));
                    return;
                }
                // Displace lower-priority geometry first. Unpublished chunks stay eligible when capacity changes.
                while self.mesh_bytes - old + bytes > self.config.mesh_cache_bytes {
                    let other = self
                        .meshes
                        .iter()
                        .filter(|(p, (_, b))| {
                            **p != c
                                && *b > 0
                                && self.plan.render_rank(**p) > self.plan.render_rank(c)
                        })
                        .max_by_key(|(p, _)| self.plan.render_rank(**p))
                        .map(|(p, _)| *p);
                    if let Some(p) = other {
                        self.discard_mesh(p, out);
                    } else {
                        self.deferred.insert(c, rev);
                        return;
                    }
                }
                if bytes < old {
                    self.deferred.clear();
                }
                self.meshes.insert(c, (rev, bytes));
                self.mesh_bytes = self.mesh_bytes - old + bytes;
                out.uploads.push(Upload {
                    chunk: c,
                    revision: rev,
                    payload,
                });
            }
            _ => unreachable!(),
        }
    }
    pub fn tick(&mut self, view: View) -> Frame<P> {
        self.tick_views(&[view])
    }
    /// One authoritative tick for all interest positions (empty releases clean residents).
    pub fn tick_views(&mut self, views: &[View]) -> Frame<P> {
        let sample_origin = views
            .first()
            .map_or([0; 3], |v| primal_voxel::chunk_at(v.block));
        let start = Instant::now();
        let mut out = Frame::default();
        if self.plan.update_views(views, &self.config, &self.world) {
            self.deferred.clear();
        }
        out.timings.push(Timing {
            chunk: sample_origin,
            stage: Stage::Select,
            duration: start.elapsed(),
        });
        let cancel = Instant::now();
        for j in self.jobs.values() {
            let cancel = match j.kind {
                Kind::Load(_) => !self.plan.wants(j.chunk),
                Kind::Mesh(r) => {
                    self.plan.render_rank(j.chunk).is_none()
                        || self.world.mesh_revision(j.chunk) != Some(r)
                }
                Kind::Save(_) => false,
            };
            if cancel
                && !j.stop.swap(true, Ordering::AcqRel)
                && let Kind::Load(t) = j.kind
            {
                self.world.cancel_load(t);
            }
        }
        out.timings.push(Timing {
            chunk: sample_origin,
            stage: Stage::Cancel,
            duration: cancel.elapsed(),
        });
        let publish = Instant::now();
        // Separate load/save completions from upload credits. Deferred meshes keep their job credit.
        while let Ok(done) = self.pool.rx.try_recv() {
            if self
                .jobs
                .get(&done.id)
                .is_some_and(|j| matches!(j.kind, Kind::Mesh(_)))
            {
                self.ready_mesh.push(done);
            } else {
                self.accept(done, &mut out);
            }
            if start.elapsed() >= self.config.main_budget {
                break;
            }
        }
        self.ready_mesh.sort_unstable_by_key(|d| {
            std::cmp::Reverse(
                self.jobs
                    .get(&d.id)
                    .and_then(|j| self.plan.render_rank(j.chunk))
                    .unwrap_or(usize::MAX),
            )
        });
        while out.uploads.len() < self.config.uploads_per_tick
            && let Some(d) = self.ready_mesh.pop()
        {
            self.accept(d, &mut out);
            if start.elapsed() >= self.config.main_budget {
                break;
            }
        }
        out.timings.push(Timing {
            chunk: sample_origin,
            stage: Stage::Publish,
            duration: publish.elapsed(),
        });
        let evict = Instant::now();
        let residents = self.world.chunks();
        for &c in &residents {
            if self.plan.render_rank(c).is_none() {
                self.discard_mesh(c, &mut out);
            }
            if !self.plan.wants(c) && self.world.evict_chunk(c) {
                self.stats.evictions += 1;
                self.errors.retain(|&(p, _)| p != c);
            }
            if self.world.chunk_dirty(c)
                && !self.active(c, 2)
                && !self.errors.contains(&(c, 2))
                && self.jobs.len() < self.config.max_jobs
            {
                let rev = self.world.chunk_revision(c).unwrap();
                self.dispatch(c, Kind::Save(rev), Work::Save(self.world.chunk_edits(c)));
            }
        }
        out.timings.push(Timing {
            chunk: sample_origin,
            stage: Stage::Evict,
            duration: evict.elapsed(),
        });
        let dispatch = Instant::now();
        self.errors.retain(|&(c, k)| k == 2 || self.plan.wants(c));
        let mut loads = self
            .jobs
            .values()
            .filter(|j| matches!(j.kind, Kind::Load(_)) && !j.stop.load(Ordering::Relaxed))
            .count();
        let mut mesh_jobs = self
            .jobs
            .values()
            .filter(|j| matches!(j.kind, Kind::Mesh(_)))
            .count();
        self.ticks = self.ticks.wrapping_add(1);
        let count = self.plan.order.len();
        // Reserve one admission every eighth tick for the tail of a stable interest area.
        // Emergency capacity (single job) remains for the body until the centre is resident.
        let fair =
            self.ticks.is_multiple_of(8) && count > 0 && self.world.is_resident(self.plan.order[0]);
        let start_cursor = self.fairness_cursor % count.max(1);
        let mut background =
            (0..if fair { count } else { 0 }).map(|offset| (start_cursor + offset) % count);
        let mut foreground = 0..count;
        let mut admitted = false;
        while let Some(i) = background.next().or_else(|| foreground.next()) {
            if self.jobs.len() >= self.config.max_jobs
                || (admitted && start.elapsed() >= self.config.main_budget)
            {
                break;
            }
            let c = self.plan.order[i];
            if !self.world.is_resident(c) {
                if self.world.resident_len() + loads < self.config.chunk_limit()
                    && !self.errors.contains(&(c, 0))
                    && let Some(t) = self.world.begin_load(c)
                {
                    self.dispatch(c, Kind::Load(t), Work::Load);
                    if !admitted && fair {
                        self.fairness_cursor = i + 1;
                        background.nth(count);
                    }
                    admitted = true;
                    loads += 1;
                }
                continue;
            }
            if self.build_meshes
                && self.plan.render_rank(c).is_some()
                && mesh_jobs < self.config.prepared_bytes / MESH_RESERVATION
                && !self.active(c, 1)
                && !self.errors.contains(&(c, 1))
            {
                let rev = self.world.mesh_revision(c).unwrap();
                if self.visible_revision(c) != Some(rev)
                    && self.deferred.get(&c) != Some(&rev)
                    && (i < 12 || self.world.chunk_dirty(c) || self.halo_resident(c))
                {
                    let snapshot = self.world.mesh_snapshot(c).unwrap();
                    self.dispatch(c, Kind::Mesh(rev), Work::Mesh(snapshot));
                    if !admitted && fair {
                        self.fairness_cursor = i + 1;
                        background.nth(count);
                    }
                    admitted = true;
                    mesh_jobs += 1;
                }
            }
        }
        out.timings.push(Timing {
            chunk: sample_origin,
            stage: Stage::Dispatch,
            duration: dispatch.elapsed(),
        });
        self.stats.peak_waiting = self.pool.peak_waiting();
        // A later load/eviction in this tick may have invalidated an accepted neighbour.
        out.uploads.retain(|u| {
            self.visible_revision(u.chunk) == Some(u.revision)
                && self.world.mesh_revision(u.chunk) == Some(u.revision)
        });
        self.stats.peak_resident = self.stats.peak_resident.max(self.world.resident_len());
        self.stats.peak_jobs = self.stats.peak_jobs.max(self.jobs.len());
        self.stats.peak_mesh_bytes = self.stats.peak_mesh_bytes.max(self.mesh_bytes);
        out
    }
}
impl Kind {
    fn code(self) -> u8 {
        match self {
            Self::Load(_) => 0,
            Self::Mesh(_) => 1,
            Self::Save(_) => 2,
        }
    }
}

impl Streamer<()> {
    /// Run the same streaming plan and storage workers without scheduling geometry.
    pub fn headless(
        world: World,
        store: impl ChunkStore,
        config: StreamConfig,
    ) -> io::Result<Self> {
        let mut streamer = Self::new(world, store, config, MeshKind::Greedy, |_| ((), 0))?;
        streamer.build_meshes = false;
        Ok(streamer)
    }
}
