//! Greedy surfaces with per-vertex ambient occlusion for opaque voxels.
use crate::mesh::{EDGE, STRIDE, adjacent_index, padded_index};
use crate::{DIRECTIONS, Face, MeshSnapshot, SIDE, Voxels};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct MaskCell {
    block: u8,
    light: [u8; 4],
}

impl MeshSnapshot {
    /// Chunk-local faces and AO levels in matching corner order (0 dark, 3 bright).
    /// A rectangle merges only if all of its vertices have the same level.
    pub fn greedy_ao_local(&self) -> Vec<(Face, [u8; 4])> {
        if matches!(self.center.as_ref(), Voxels::Uniform(0)) {
            return Vec::new();
        }
        let grid = self.ao_blocks();
        let mut faces = Vec::new();
        let empty = MaskCell::default();
        let mut mask = [empty; SIDE * SIDE];
        for (side, normal) in DIRECTIONS.into_iter().enumerate() {
            let axis = side / 2;
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let positive = normal[axis] > 0;
            for slice in 0..SIDE {
                let first = padded_index([0; 3]) + slice * STRIDE[axis];
                for row in 0..SIDE {
                    let start = first + row * STRIDE[v];
                    for col in 0..SIDE {
                        let i = start + col * STRIDE[u];
                        let air = adjacent_index(i, side);
                        mask[row * SIDE + col] = if grid[i] != 0 && grid[air] == 0 {
                            MaskCell {
                                block: grid[i],
                                light: face_light(&grid, air, STRIDE[u], STRIDE[v]),
                            }
                        } else {
                            empty
                        };
                    }
                }
                for row in 0..SIDE {
                    let mut col = 0;
                    while col < SIDE {
                        let cell = mask[row * SIDE + col];
                        if cell.block == 0 {
                            col += 1;
                            continue;
                        }
                        let (mut w, mut h) = (1, 1);
                        if cell.light == [cell.light[0]; 4] {
                            while col + w < SIDE && mask[row * SIDE + col + w] == cell {
                                w += 1;
                            }
                            while row + h < SIDE
                                && mask[(row + h) * SIDE + col..(row + h) * SIDE + col + w]
                                    .iter()
                                    .all(|&c| c == cell)
                            {
                                h += 1;
                            }
                        }
                        for r in row..row + h {
                            mask[r * SIDE + col..r * SIDE + col + w].fill(empty);
                        }
                        let mut start = [0.0; 3];
                        start[axis] = (slice + usize::from(positive)) as f32;
                        start[u] = col as f32;
                        start[v] = row as f32;
                        let mut along_u = start;
                        along_u[u] += w as f32;
                        let mut along_v = start;
                        along_v[v] += h as f32;
                        let mut opposite = along_u;
                        opposite[v] = along_v[v];
                        let mut corners = [start, along_u, opposite, along_v];
                        let mut light = cell.light;
                        if !positive {
                            corners.swap(1, 3);
                            light.swap(1, 3);
                        }
                        faces.push((
                            Face {
                                corners,
                                normal,
                                block: cell.block,
                            },
                            light,
                        ));
                        col += w;
                    }
                }
            }
        }
        faces
    }
}

fn face_light(grid: &[u8; EDGE * EDGE * EDGE], air: usize, du: usize, dv: usize) -> [u8; 4] {
    let along_u = [air - du, air + du];
    let along_v = [air - dv, air + dv];
    [(0, 0), (1, 0), (1, 1), (0, 1)].map(|(u, v)| {
        let side_u = grid[along_u[u]] != 0;
        let side_v = grid[along_v[v]] != 0;
        let corner = if v == 0 {
            along_u[u] - dv
        } else {
            along_u[u] + dv
        };
        if side_u && side_v {
            0
        } else {
            3 - u8::from(side_u) - u8::from(side_v) - u8::from(grid[corner] != 0)
        }
    })
}
