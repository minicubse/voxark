//! Renderer-independent voxel generation, resident chunks and versioned mesh views.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

mod binary;
mod catalog;
mod forest;
mod generation;
mod mesh;
pub use catalog::{BlockDef, CatalogError, block_def, parse_block_catalog};
pub use generation::{
    GeneratedChunk, Preset, generate_chunk, generate_for, surface_for, surface_hint,
};
pub use mesh::{Face, MeshSnapshot};

pub const CHUNK: i32 = 16;
pub const MAX_BLOCK: u8 = 7;
/// Terrain inside the valley is the original generator, including its structures.
pub const GENERATOR_VERSION: u32 = 1;
pub const STREAM_HEIGHT: i32 = 128;
pub type Pos = [i32; 3];
pub type ChunkPos = [i32; 3];
/// Exclusive upper bounds. These constants describe the historical valley.
pub const WORLD_MIN: Pos = [-64, 0, -64];
pub const WORLD_MAX: Pos = [64, 64, 64];

const SIDE: usize = CHUNK as usize;
const VOLUME: usize = SIDE * SIDE * SIDE;
const DIRECTIONS: [Pos; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
];

pub fn chunk_at(position: Pos) -> ChunkPos {
    position.map(|v| v.div_euclid(CHUNK))
}

fn local_index([x, y, z]: Pos) -> usize {
    ((y.rem_euclid(CHUNK) * CHUNK + z.rem_euclid(CHUNK)) * CHUNK + x.rem_euclid(CHUNK)) as usize
}

fn position_at(chunk: ChunkPos, index: u16) -> Pos {
    let i = i32::from(index);
    let [x, y, z] = chunk.map(|v| v * CHUNK);
    [
        x + i % CHUNK,
        y + i / (CHUNK * CHUNK),
        z + (i / CHUNK) % CHUNK,
    ]
}

fn stream_chunk([x, y, z]: ChunkPos) -> bool {
    let horizontal = (i32::MIN / CHUNK)..=(i32::MAX / CHUNK);
    horizontal.contains(&x) && (0..STREAM_HEIGHT / CHUNK).contains(&y) && horizontal.contains(&z)
}

fn neighbour(chunk: ChunkPos, direction: Pos) -> ChunkPos {
    // Valid chunk coordinates are always at least one unit inside i32.
    std::array::from_fn(|i| chunk[i] + direction[i])
}

#[derive(Clone)]
enum Voxels {
    Uniform(u8),
    Dense(Box<[u8; VOLUME]>),
}

impl Voxels {
    fn get(&self, index: usize) -> u8 {
        match self {
            Self::Uniform(block) => *block,
            Self::Dense(cells) => cells[index],
        }
    }

    fn set(&mut self, index: usize, block: u8) {
        if let Self::Uniform(value) = self {
            *self = Self::Dense(Box::new([*value; VOLUME]));
        }
        if let Self::Dense(cells) = self {
            cells[index] = block;
        }
    }

    fn payload_bytes(&self) -> usize {
        match self {
            Self::Uniform(_) => 1,
            Self::Dense(_) => VOLUME,
        }
    }
}

struct Resident {
    cells: Arc<Voxels>,
    /// Local index -> (original generated value, current value).
    differences: BTreeMap<u16, (u8, u8)>,
    revision: u64,
    mesh_revision: u64,
    dirty: bool,
}

impl Resident {
    fn replace(&mut self, local: usize, block: u8) {
        let original = self
            .differences
            .get(&(local as u16))
            .map_or_else(|| self.cells.get(local), |&(original, _)| original);
        if original == block {
            self.differences.remove(&(local as u16));
        } else {
            self.differences.insert(local as u16, (original, block));
        }
        Arc::make_mut(&mut self.cells).set(local, block);
    }
}

/// A request is spent by installation or cancellation. A later request for the
/// same coordinate has a different id, even after eviction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LoadTicket {
    chunk: ChunkPos,
    id: u64,
}

impl LoadTicket {
    pub fn chunk(self) -> ChunkPos {
        self.chunk
    }
    pub fn id(self) -> u64 {
        self.id
    }
}

pub struct World {
    seed: u64,
    preset: Preset,
    valley: bool,
    chunks: HashMap<ChunkPos, Resident>,
    pending: HashMap<ChunkPos, u64>,
    serial: u64,
}

impl World {
    /// Historical finite fixture and v1 save reader. Runtime streaming starts
    /// from `streamed` and only generates the chunks requested by the caller.
    pub fn new(seed: u64) -> Self {
        let mut world = Self::valley_streamed(seed);
        for y in 0..4 {
            for z in -4..4 {
                for x in -4..4 {
                    let chunk = [x, y, z];
                    let ticket = world.begin_load(chunk).expect("valley chunk");
                    let data = generate_chunk(seed, chunk).expect("valley generation");
                    assert!(world.install_chunk(ticket, data, &[]));
                }
            }
        }
        world
    }

    /// Empty finite domain for streaming the historical valley in comparative benchmarks.
    pub fn valley_streamed(seed: u64) -> Self {
        let mut world = Self::streamed(seed);
        world.valley = true;
        world
    }

    pub fn streamed(seed: u64) -> Self {
        Self::with_preset(seed, Preset::Historical)
    }

    /// Empty streamed world whose chunks are generated with the selected preset.
    pub fn with_preset(seed: u64, preset: Preset) -> Self {
        Self {
            seed,
            preset,
            valley: false,
            chunks: HashMap::new(),
            pending: HashMap::new(),
            serial: 0,
        }
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn preset(&self) -> Preset {
        self.preset
    }

    /// Legacy footprint used to validate PVXD v1 records.
    pub fn contains(p: Pos) -> bool {
        (0..3).all(|i| (WORLD_MIN[i]..WORLD_MAX[i]).contains(&p[i]))
    }

    pub fn in_bounds(&self, p: Pos) -> bool {
        if self.valley {
            Self::contains(p)
        } else {
            (0..STREAM_HEIGHT).contains(&p[1])
        }
    }

    /// Containment bounds in block coordinates, with exclusive upper endpoints.
    /// i64 can express the exclusive upper endpoint above i32::MAX.
    pub fn block_bounds(&self) -> ([i64; 3], [i64; 3]) {
        if self.valley {
            (WORLD_MIN.map(i64::from), WORLD_MAX.map(i64::from))
        } else {
            (
                [i64::from(i32::MIN), 0, i64::from(i32::MIN)],
                [
                    i64::from(i32::MAX) + 1,
                    i64::from(STREAM_HEIGHT),
                    i64::from(i32::MAX) + 1,
                ],
            )
        }
    }

    pub fn contains_chunk(&self, chunk: ChunkPos) -> bool {
        stream_chunk(chunk) && (!self.valley || Self::contains(chunk.map(|v| v * CHUNK)))
    }

    pub fn resident_len(&self) -> usize {
        self.chunks.len()
    }
    pub fn is_resident(&self, chunk: ChunkPos) -> bool {
        self.chunks.contains_key(&chunk)
    }

    /// Logical block and sparse-difference payload. Excludes allocation/index
    /// overhead, worker-owned snapshots, queues and renderer memory.
    pub fn resident_bytes(&self) -> usize {
        self.chunks
            .values()
            .map(|c| c.cells.payload_bytes() + c.differences.len() * size_of::<(u16, (u8, u8))>())
            .sum()
    }

    pub fn chunks(&self) -> Vec<ChunkPos> {
        let mut chunks: Vec<_> = self.chunks.keys().copied().collect();
        chunks.sort_unstable_by_key(|&[x, y, z]| (y, z, x));
        chunks
    }

    fn next_revision(&mut self) -> u64 {
        self.serial = self
            .serial
            .checked_add(1)
            .expect("world revision exhausted");
        self.serial
    }

    pub fn begin_load(&mut self, chunk: ChunkPos) -> Option<LoadTicket> {
        if !self.contains_chunk(chunk)
            || self.is_resident(chunk)
            || self.pending.contains_key(&chunk)
        {
            return None;
        }
        let id = self.next_revision();
        self.pending.insert(chunk, id);
        Some(LoadTicket { chunk, id })
    }

    pub fn cancel_load(&mut self, ticket: LoadTicket) -> bool {
        if self.pending.get(&ticket.chunk) != Some(&ticket.id) {
            return false;
        }
        self.pending.remove(&ticket.chunk);
        true
    }

    /// Publish an unedited generator result plus its persisted differences.
    /// Invalid persisted data consumes its own ticket and publishes nothing.
    pub fn install_chunk(
        &mut self,
        ticket: LoadTicket,
        data: GeneratedChunk,
        edits: &[(Pos, u8)],
    ) -> bool {
        if !self.cancel_load(ticket)
            || data.chunk != ticket.chunk
            || data.seed != self.seed
            || data.preset != self.preset
            || edits.len() > VOLUME
        {
            return false;
        }
        let revision = self.next_revision();
        let mut resident = Resident {
            cells: Arc::new(data.cells),
            differences: BTreeMap::new(),
            revision,
            mesh_revision: revision,
            dirty: false,
        };
        for &(pos, block) in edits {
            let index = local_index(pos);
            if block > MAX_BLOCK
                || chunk_at(pos) != ticket.chunk
                || resident.differences.contains_key(&(index as u16))
                || resident.cells.get(index) == block
            {
                return false;
            }
            resident.replace(index, block);
        }
        self.chunks.insert(ticket.chunk, resident);
        self.invalidate_neighbours(ticket.chunk, revision);
        true
    }

    fn invalidate_neighbours(&mut self, chunk: ChunkPos, revision: u64) {
        for normal in DIRECTIONS {
            if let Some(adjacent) = self.chunks.get_mut(&neighbour(chunk, normal)) {
                adjacent.mesh_revision = revision;
            }
        }
    }

    pub fn chunk_revision(&self, chunk: ChunkPos) -> Option<u64> {
        self.chunks.get(&chunk).map(|c| c.revision)
    }

    pub fn mesh_revision(&self, chunk: ChunkPos) -> Option<u64> {
        self.chunks.get(&chunk).map(|c| c.mesh_revision)
    }

    pub fn chunk_dirty(&self, chunk: ChunkPos) -> bool {
        self.chunks.get(&chunk).is_some_and(|c| c.dirty)
    }

    /// A delayed write acknowledgement never clears a newer change.
    pub fn mark_saved(&mut self, chunk: ChunkPos, revision: u64) -> bool {
        let Some(c) = self.chunks.get_mut(&chunk) else {
            return false;
        };
        if c.revision != revision {
            return false;
        }
        c.dirty = false;
        true
    }

    pub fn evict_chunk(&mut self, chunk: ChunkPos) -> bool {
        if !self.chunks.get(&chunk).is_some_and(|c| !c.dirty) {
            return false;
        }
        self.chunks.remove(&chunk);
        let revision = self.next_revision();
        self.invalidate_neighbours(chunk, revision);
        true
    }

    /// Some(0) is known air. Unknown and out-of-domain cells return None.
    pub fn lookup(&self, position: Pos) -> Option<u8> {
        self.chunks
            .get(&chunk_at(position))
            .map(|chunk| chunk.cells.get(local_index(position)))
    }

    /// Historical sampler for the finite valley. Collisions and interactions
    /// use `lookup` to distinguish unloaded data from air.
    pub fn get(&self, position: Pos) -> u8 {
        self.lookup(position).unwrap_or(0)
    }

    pub fn set(&mut self, position: Pos, block: u8) -> bool {
        if block > MAX_BLOCK {
            return false;
        }
        let chunk = chunk_at(position);
        let index = local_index(position);
        let Some(resident) = self.chunks.get(&chunk) else {
            return false;
        };
        if resident.cells.get(index) == block {
            return false;
        }
        let revision = self.next_revision();
        let resident = self.chunks.get_mut(&chunk).expect("resident chunk");
        resident.replace(index, block);
        resident.revision = revision;
        resident.mesh_revision = revision;
        resident.dirty = true;
        for face in touched_faces(position) {
            if let Some(c) = self.chunks.get_mut(&neighbour(chunk, face)) {
                c.mesh_revision = revision;
            }
        }
        true
    }

    pub fn chunk_edits(&self, chunk: ChunkPos) -> Vec<(Pos, u8)> {
        let Some(c) = self.chunks.get(&chunk) else {
            return Vec::new();
        };
        let mut edits: Vec<_> = c
            .differences
            .iter()
            .map(|(&i, &(_, block))| (position_at(chunk, i), block))
            .collect();
        edits.sort_unstable_by_key(|&(p, _)| p);
        edits
    }

    /// Canonical PVXD v1 order; streamed saves only need `chunk_edits`.
    pub fn edits(&self) -> Vec<(Pos, u8)> {
        let mut edits: Vec<_> = self
            .chunks
            .iter()
            .flat_map(|(&chunk, c)| {
                c.differences
                    .iter()
                    .map(move |(&i, &(_, block))| (position_at(chunk, i), block))
            })
            .collect();
        edits.sort_unstable_by_key(|&(p, _)| p);
        edits
    }

    pub fn solid_count(&self) -> usize {
        self.chunks
            .values()
            .map(|c| match c.cells.as_ref() {
                Voxels::Uniform(0) => 0,
                Voxels::Uniform(_) => VOLUME,
                Voxels::Dense(bytes) => bytes.iter().filter(|&&b| b != 0).count(),
            })
            .sum()
    }

    pub fn dirty_chunks(&self, position: Pos) -> Vec<ChunkPos> {
        affected(position, |c| self.contains_chunk(c))
    }

    pub fn mesh_snapshot(&self, chunk: ChunkPos) -> Option<MeshSnapshot> {
        let c = self.chunks.get(&chunk)?;
        Some(MeshSnapshot {
            chunk,
            revision: c.mesh_revision,
            center: c.cells.clone(),
            adjacent: DIRECTIONS.map(|d| {
                self.chunks
                    .get(&neighbour(chunk, d))
                    .map(|n| n.cells.clone())
            }),
        })
    }

    pub fn mesh(&self, chunk: ChunkPos) -> Vec<Face> {
        self.mesh_snapshot(chunk)
            .map_or_else(Vec::new, |s| s.mesh())
    }

    pub fn greedy_mesh(&self, chunk: ChunkPos) -> Vec<Face> {
        self.mesh_snapshot(chunk)
            .map_or_else(Vec::new, |s| s.greedy_mesh())
    }
}

fn touched_faces(position: Pos) -> impl Iterator<Item = Pos> {
    (0..3).filter_map(move |axis| {
        let side = match position[axis].rem_euclid(CHUNK) {
            0 => 1,
            v if v == CHUNK - 1 => 0,
            _ => return None,
        };
        Some(DIRECTIONS[axis * 2 + side])
    })
}

fn affected(position: Pos, valid: impl Fn(ChunkPos) -> bool) -> Vec<ChunkPos> {
    let chunk = chunk_at(position);
    if !valid(chunk) {
        return Vec::new();
    }
    let mut dirty = vec![chunk];
    dirty.extend(
        touched_faces(position)
            .map(|d| neighbour(chunk, d))
            .filter(|&c| valid(c)),
    );
    dirty
}

/// Historical helper restricted to the finite valley. Streamed callers use
/// `world.dirty_chunks(position)` with the bounds of their actual world.
pub fn dirty_chunks(position: Pos) -> Vec<ChunkPos> {
    affected(position, |c| {
        stream_chunk(c) && World::contains(c.map(|v| v * CHUNK))
    })
}
