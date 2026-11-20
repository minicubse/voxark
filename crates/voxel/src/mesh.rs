use super::{CHUNK, ChunkPos, DIRECTIONS, SIDE, Voxels};
use std::sync::Arc;

const EDGE: usize = SIDE + 2;
const STRIDE: [usize; 3] = [1, EDGE * EDGE, EDGE];
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

/// Immutable block and face-neighbour references captured in one world update.
/// Edits while a worker is running copy only the affected shared voxel buffer.
#[derive(Clone)]
pub struct MeshSnapshot {
    pub(super) chunk: ChunkPos,
    pub(super) revision: u64,
    pub(super) center: Arc<Voxels>,
    pub(super) adjacent: [Option<Arc<Voxels>>; 6],
}

impl MeshSnapshot {
    pub fn chunk(&self) -> ChunkPos {
        self.chunk
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Row-major Y, Z, X with a one-cell halo, X contiguous. The six face
    /// planes are sufficient for both meshers. Missing neighbours expose the
    /// frontier until their arrival invalidates this snapshot's revision.
    pub fn padded_blocks(&self) -> [u8; EDGE * EDGE * EDGE] {
        let mut grid = [0; EDGE * EDGE * EDGE];
        for y in 0..SIDE {
            for z in 0..SIDE {
                let row = padded_index([0, y, z]);
                let from = (y * SIDE + z) * SIDE;
                let dst = &mut grid[row..row + SIDE];
                match self.center.as_ref() {
                    Voxels::Uniform(v) => dst.fill(*v),
                    Voxels::Dense(cells) => dst.copy_from_slice(&cells[from..from + SIDE]),
                }
            }
        }
        for (side, neighbour) in self.adjacent.iter().enumerate() {
            let Some(cells) = neighbour else { continue };
            if matches!(cells.as_ref(), Voxels::Uniform(0)) {
                continue;
            }
            let axis = side / 2;
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let positive = side % 2 == 0;
            let mut src = [0; 3];
            src[axis] = if positive { 0 } else { SIDE - 1 };
            for row in 0..SIDE {
                src[v] = row;
                for col in 0..SIDE {
                    src[u] = col;
                    let mut dst = src.map(|n| n + 1);
                    dst[axis] = if positive { EDGE - 1 } else { 0 };
                    grid[(dst[1] * EDGE + dst[2]) * EDGE + dst[0]] =
                        cells.get((src[1] * SIDE + src[2]) * SIDE + src[0]);
                }
            }
        }
        grid
    }

    pub fn mesh(&self) -> Vec<Face> {
        if matches!(self.center.as_ref(), Voxels::Uniform(0)) {
            return Vec::new();
        }
        let grid = self.padded_blocks();
        let origin = self.chunk.map(|c| i64::from(c) * i64::from(CHUNK));
        let mut faces = Vec::new();
        for y in 0..SIDE {
            for z in 0..SIDE {
                for x in 0..SIDE {
                    let p = [x, y, z];
                    let index = padded_index(p);
                    let block = grid[index];
                    if block == 0 {
                        continue;
                    }
                    for (side, normal) in DIRECTIONS.into_iter().enumerate() {
                        if grid[adjacent_index(index, side)] == 0 {
                            faces.push(Face {
                                corners: CORNERS[side].map(|corner| {
                                    std::array::from_fn(|a| {
                                        (origin[a] + p[a] as i64 + i64::from(corner[a])) as f32
                                    })
                                }),
                                normal,
                                block,
                            });
                        }
                    }
                }
            }
        }
        faces
    }

    pub fn greedy_mesh(&self) -> Vec<Face> {
        if matches!(self.center.as_ref(), Voxels::Uniform(0)) {
            return Vec::new();
        }
        let grid = self.padded_blocks();
        let origin = self.chunk.map(|v| i64::from(v) * i64::from(CHUNK));
        let mut faces = Vec::new();
        let mut mask = [0; SIDE * SIDE];
        for (side, normal) in DIRECTIONS.into_iter().enumerate() {
            let axis = side / 2;
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            for slice in 0..SIDE {
                let first = padded_index([0; 3]) + slice * STRIDE[axis];
                for row in 0..SIDE {
                    let start = first + row * STRIDE[v];
                    for col in 0..SIDE {
                        let i = start + col * STRIDE[u];
                        mask[row * SIDE + col] = if grid[adjacent_index(i, side)] == 0 {
                            grid[i]
                        } else {
                            0
                        };
                    }
                }
                for row in 0..SIDE {
                    let mut col = 0;
                    while col < SIDE {
                        let block = mask[row * SIDE + col];
                        if block == 0 {
                            col += 1;
                            continue;
                        }
                        let mut w = 1;
                        while col + w < SIDE && mask[row * SIDE + col + w] == block {
                            w += 1;
                        }
                        let mut h = 1;
                        while row + h < SIDE
                            && mask[(row + h) * SIDE + col..(row + h) * SIDE + col + w]
                                .iter()
                                .all(|&b| b == block)
                        {
                            h += 1;
                        }
                        for r in row..row + h {
                            mask[r * SIDE + col..r * SIDE + col + w].fill(0);
                        }
                        let mut start = origin;
                        start[axis] += slice as i64 + i64::from(normal[axis] > 0);
                        start[u] += col as i64;
                        start[v] += row as i64;
                        let mut along_u = start;
                        along_u[u] += w as i64;
                        let mut along_v = start;
                        along_v[v] += h as i64;
                        let mut opposite = along_u;
                        opposite[v] += h as i64;
                        let corners = if normal[axis] > 0 {
                            [start, along_u, opposite, along_v]
                        } else {
                            [start, along_v, opposite, along_u]
                        };
                        faces.push(Face {
                            corners: corners.map(|corner| corner.map(|n| n as f32)),
                            normal,
                            block,
                        });
                        col += w;
                    }
                }
            }
        }
        faces
    }
}

#[inline]
fn padded_index([x, y, z]: [usize; 3]) -> usize {
    ((y + 1) * EDGE + z + 1) * EDGE + x + 1
}

#[inline]
fn adjacent_index(i: usize, side: usize) -> usize {
    if side.is_multiple_of(2) {
        i + STRIDE[side / 2]
    } else {
        i - STRIDE[side / 2]
    }
}
