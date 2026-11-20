//! Deterministic finite voxel world and renderer-independent exposed-face meshes.

use std::collections::BTreeMap;

mod catalog;
pub use catalog::{BlockDef, CatalogError, block_def, parse_block_catalog};

pub const CHUNK: i32 = 16;
pub const MAX_BLOCK: u8 = 7;
pub const GENERATOR_VERSION: u32 = 1;
pub type Pos = [i32; 3];
pub type ChunkPos = [i32; 3];
pub const WORLD_MIN: Pos = [-64, 0, -64];
pub const WORLD_MAX: Pos = [64, 64, 64];

const CHUNK_VOLUME: usize = 16 * 16 * 16;
const DIRECTIONS: [Pos; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
];
const CORNERS: [[[i32; 3]; 4]; 6] = [
    [[1, 0, 0], [1, 1, 0], [1, 1, 1], [1, 0, 1]],
    [[0, 0, 1], [0, 1, 1], [0, 1, 0], [0, 0, 0]],
    [[0, 1, 1], [1, 1, 1], [1, 1, 0], [0, 1, 0]],
    [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]],
    [[1, 0, 1], [1, 1, 1], [0, 1, 1], [0, 0, 1]],
    [[0, 0, 0], [0, 1, 0], [1, 1, 0], [1, 0, 0]],
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face {
    pub corners: [[f32; 3]; 4],
    pub normal: [i32; 3],
    pub block: u8,
}

/// Dense 16³ chunks form the correctness and performance baseline.
/// Original data allows sparse edits to normalize when a block is restored.
pub struct World {
    seed: u64,
    chunks: Vec<Vec<u8>>,
    original: Vec<Vec<u8>>,
    edits: BTreeMap<Pos, u8>,
}

impl World {
    pub fn new(seed: u64) -> Self {
        let mut world = Self {
            seed,
            chunks: vec![vec![0; CHUNK_VOLUME]; 256],
            original: Vec::new(),
            edits: BTreeMap::new(),
        };
        world.generate();
        world.original.clone_from(&world.chunks);
        world
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn contains([x, y, z]: Pos) -> bool {
        (WORLD_MIN[0]..WORLD_MAX[0]).contains(&x)
            && (WORLD_MIN[1]..WORLD_MAX[1]).contains(&y)
            && (WORLD_MIN[2]..WORLD_MAX[2]).contains(&z)
    }

    pub fn get(&self, position: Pos) -> u8 {
        address(position).map_or(0, |(chunk, local)| self.chunks[chunk][local])
    }

    /// Returns true only when a valid edit actually changes the world.
    pub fn set(&mut self, position: Pos, block: u8) -> bool {
        if block > MAX_BLOCK {
            return false;
        }
        let Some((chunk, local)) = address(position) else {
            return false;
        };
        if self.chunks[chunk][local] == block {
            return false;
        }
        self.chunks[chunk][local] = block;
        if self.original[chunk][local] == block {
            self.edits.remove(&position);
        } else {
            self.edits.insert(position, block);
        }
        true
    }

    pub fn chunks(&self) -> Vec<ChunkPos> {
        let mut chunks = Vec::with_capacity(256);
        for y in 0..4 {
            for z in -4..4 {
                for x in -4..4 {
                    chunks.push([x, y, z]);
                }
            }
        }
        chunks
    }

    pub fn edits(&self) -> Vec<(Pos, u8)> {
        self.edits
            .iter()
            .map(|(&position, &block)| (position, block))
            .collect()
    }

    pub fn solid_count(&self) -> usize {
        self.chunks
            .iter()
            .flatten()
            .filter(|&&block| block != 0)
            .count()
    }

    /// Includes neighbour chunks when culling, so internal seams have no faces.
    pub fn mesh(&self, chunk: ChunkPos) -> Vec<Face> {
        if !valid_chunk(chunk) {
            return Vec::new();
        }
        let origin = chunk.map(|coordinate| coordinate * CHUNK);
        let mut faces = Vec::new();
        for y in 0..CHUNK {
            for z in 0..CHUNK {
                for x in 0..CHUNK {
                    let position = [origin[0] + x, origin[1] + y, origin[2] + z];
                    let block = self.get(position);
                    if block == 0 {
                        continue;
                    }
                    for (side, normal) in DIRECTIONS.iter().enumerate() {
                        let neighbour = std::array::from_fn(|axis| position[axis] + normal[axis]);
                        if self.get(neighbour) == 0 {
                            faces.push(Face {
                                corners: CORNERS[side].map(|corner| {
                                    std::array::from_fn(|axis| {
                                        (position[axis] + corner[axis]) as f32
                                    })
                                }),
                                normal: *normal,
                                block,
                            });
                        }
                    }
                }
            }
        }
        faces
    }

    /// Merges coplanar exposed faces of the same material within one chunk.
    /// Neighbour sampling is shared with the unit-face reference mesh.
    pub fn greedy_mesh(&self, chunk: ChunkPos) -> Vec<Face> {
        if !valid_chunk(chunk) {
            return Vec::new();
        }
        let origin = chunk.map(|coordinate| coordinate * CHUNK);
        let (index, _) = address(origin).expect("validated chunk origin");
        if self.chunks[index].iter().all(|&block| block == 0) {
            return Vec::new();
        }
        let mut faces = Vec::new();
        let mut mask = [0_u8; 256];
        for (side, normal) in DIRECTIONS.iter().enumerate() {
            let axis = side / 2;
            let u = (axis + 1) % 3;
            let v = (axis + 2) % 3;
            for slice in 0..CHUNK {
                for row in 0..CHUNK {
                    for column in 0..CHUNK {
                        let mut position = origin;
                        position[axis] += slice;
                        position[u] += column;
                        position[v] += row;
                        let block = self.get(position);
                        let mut neighbour = position;
                        neighbour[axis] += normal[axis];
                        mask[(row * CHUNK + column) as usize] =
                            if block != 0 && self.get(neighbour) == 0 {
                                block
                            } else {
                                0
                            };
                    }
                }
                for row in 0..CHUNK {
                    let mut column = 0;
                    while column < CHUNK {
                        let block = mask[(row * CHUNK + column) as usize];
                        if block == 0 {
                            column += 1;
                            continue;
                        }
                        let mut width = 1;
                        while column + width < CHUNK
                            && mask[(row * CHUNK + column + width) as usize] == block
                        {
                            width += 1;
                        }
                        let mut height = 1;
                        while row + height < CHUNK
                            && (column..column + width).all(|offset| {
                                mask[((row + height) * CHUNK + offset) as usize] == block
                            })
                        {
                            height += 1;
                        }
                        for clear_row in row..row + height {
                            for clear_column in column..column + width {
                                mask[(clear_row * CHUNK + clear_column) as usize] = 0;
                            }
                        }
                        let mut start = origin;
                        start[axis] += slice + i32::from(normal[axis] > 0);
                        start[u] += column;
                        start[v] += row;
                        let mut along_u = start;
                        along_u[u] += width;
                        let mut along_v = start;
                        along_v[v] += height;
                        let mut opposite = along_u;
                        opposite[v] += height;
                        let corners = if normal[axis] > 0 {
                            [start, along_u, opposite, along_v]
                        } else {
                            [start, along_v, opposite, along_u]
                        };
                        faces.push(Face {
                            corners: corners.map(|corner| corner.map(|value| value as f32)),
                            normal: *normal,
                            block,
                        });
                        column += width;
                    }
                }
            }
        }
        faces
    }

    fn generate(&mut self) {
        for z in -64..64 {
            for x in -64..64 {
                let height = self.height(x, z);
                for y in 0..=height {
                    let block = if y < height - 3 {
                        3
                    } else if y < height {
                        2
                    } else if height <= 8 {
                        4
                    } else {
                        1
                    };
                    self.write_generated([x, y, z], block);
                }
            }
        }

        // Sparse umbrella trees keep the central walking route clear.
        for grid_z in -4..=4 {
            for grid_x in -4..=4 {
                let random = hash(self.seed, grid_x, grid_z);
                let x = grid_x * 13 + (random % 7) as i32 - 3;
                let z = grid_z * 13 + ((random >> 8) % 7) as i32 - 3;
                if x.abs() < 10 || !(-59..59).contains(&x) || !(-59..59).contains(&z) {
                    continue;
                }
                let ground = self.height(x, z);
                if !(9..=26).contains(&ground) || random.is_multiple_of(4) {
                    continue;
                }
                let top = ground + 7 + ((random >> 16) % 3) as i32;
                for y in ground + 1..=top {
                    self.write_generated([x, y, z], 5);
                }
                for dy in -1_i32..=2 {
                    for dz in -4_i32..=4 {
                        for dx in -4_i32..=4 {
                            let radius = 5 - dy.max(0);
                            if dx.abs() + dz.abs() <= radius && (dx != 0 || dz != 0 || dy > 0) {
                                self.write_generated([x + dx, top + dy, z + dz], 6);
                            }
                        }
                    }
                }
            }
        }

        // An original basalt gateway frames the valley's distant skyline.
        for x in -27_i32..=-12 {
            let distance = (x + 20).abs();
            for z in 34..=37 {
                for y in 12..=32 {
                    let pillar = distance >= 5 && y < 30;
                    let lintel = (28..=31).contains(&y) && distance <= 8;
                    if pillar || lintel {
                        self.write_generated([x, y, z], 3);
                    }
                }
            }
        }
        for (cx, cz) in [(-15, 24), (23, 38), (-38, -24)] {
            let base = self.height(cx, cz) + 1;
            for dz in -2_i32..=2 {
                for dx in -2_i32..=2 {
                    let height = 8 - (dx.abs() + dz.abs()) * 2;
                    for dy in 0..height {
                        self.write_generated([cx + dx, base + dy, cz + dz], 7);
                    }
                }
            }
        }
    }

    fn height(&self, x: i32, z: i32) -> i32 {
        let rolling = noise(self.seed, x, z, 16, 5) + noise(self.seed ^ 517, x, z, 7, 2);
        let left_peak = (29 - ((x + 43).abs() + (z - 43).abs()) * 2 / 3).max(0);
        let right_peak = (32 - ((x - 43).abs() + (z - 44).abs()) * 2 / 3).max(0);
        let valley_wall = ((x.abs() - 23).max(0) * 2 / 5).min(15);
        (8 + rolling + valley_wall + left_peak.max(right_peak)).min(47)
    }

    fn write_generated(&mut self, position: Pos, block: u8) {
        if let Some((chunk, local)) = address(position) {
            self.chunks[chunk][local] = block;
        }
    }
}

fn valid_chunk([x, y, z]: ChunkPos) -> bool {
    (-4..4).contains(&x) && (0..4).contains(&y) && (-4..4).contains(&z)
}

fn address([x, y, z]: Pos) -> Option<(usize, usize)> {
    if !World::contains([x, y, z]) {
        return None;
    }
    let cx = (x + 64) / CHUNK;
    let cy = y / CHUNK;
    let cz = (z + 64) / CHUNK;
    let chunk = ((cy * 8 + cz) * 8 + cx) as usize;
    let local = ((y.rem_euclid(CHUNK) * CHUNK + z.rem_euclid(CHUNK)) * CHUNK + x.rem_euclid(CHUNK))
        as usize;
    Some((chunk, local))
}

/// Neighbours are needed only on touched faces, including negative coordinates.
pub fn dirty_chunks(position: Pos) -> Vec<ChunkPos> {
    if !World::contains(position) {
        return Vec::new();
    }
    let chunk = position.map(|coordinate| coordinate.div_euclid(CHUNK));
    let mut dirty = vec![chunk];
    for axis in 0..3 {
        let local = position[axis].rem_euclid(CHUNK);
        if local == 0 || local == CHUNK - 1 {
            let mut neighbour = chunk;
            neighbour[axis] += if local == 0 { -1 } else { 1 };
            if valid_chunk(neighbour) {
                dirty.push(neighbour);
            }
        }
    }
    dirty
}

fn hash(seed: u64, x: i32, z: i32) -> u64 {
    let mut value = seed
        ^ (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (z as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

// Integer bilinear noise gives identical terrain across platforms.
fn noise(seed: u64, x: i32, z: i32, scale: i32, amplitude: i32) -> i32 {
    let gx = x.div_euclid(scale);
    let gz = z.div_euclid(scale);
    let tx = x.rem_euclid(scale);
    let tz = z.rem_euclid(scale);
    let sample = |dx, dz| (hash(seed, gx + dx, gz + dz) % 1024) as i32;
    let top = sample(0, 0) * (scale - tx) + sample(1, 0) * tx;
    let bottom = sample(0, 1) * (scale - tx) + sample(1, 1) * tx;
    ((top * (scale - tz) + bottom * tz) * amplitude) / (scale * scale * 1024)
}
