//! Binary visibility and rectangle merging for opaque, axis-aligned voxel faces.
//! Kept as a separate mesher so both the unit-face oracle and scalar greedy path remain usable.
use crate::{CHUNK, Face, MeshSnapshot};

const N: usize = CHUNK as usize;
const HALO: usize = N + 2;
// Padded voxel layout: x advances first, followed by z, then y.
const STRIDE: [usize; 3] = [1, HALO * HALO, HALO];

impl MeshSnapshot {
    pub fn binary_greedy_mesh(&self) -> Vec<Face> {
        let mut faces = Vec::new();
        self.binary_greedy_into(&mut faces);
        faces
    }

    /// Clear the previous output while retaining its allocation for this worker's next job.
    pub fn binary_greedy_into(&self, faces: &mut Vec<Face>) {
        faces.clear();
        if matches!(self.center.as_ref(), crate::Voxels::Uniform(0)) {
            return;
        }
        let blocks = self.padded_blocks();
        let origin = self.chunk().map(|v| i64::from(v) * i64::from(CHUNK));
        let center = STRIDE.iter().sum::<usize>();
        for axis in 0..3 {
            let u = (axis + 1) % 3;
            let v = (axis + 2) % 3;
            // One 16-bit occupancy row per plane, including the two neighbouring planes.
            let mut solid = [[0_u16; N]; HALO];
            for (plane, rows) in solid.iter_mut().enumerate() {
                for (row, bits) in rows.iter_mut().enumerate() {
                    let first = center - STRIDE[axis] + plane * STRIDE[axis] + row * STRIDE[v];
                    for column in 0..N {
                        *bits |= u16::from(blocks[first + column * STRIDE[u]] != 0) << column;
                    }
                }
            }
            for sign in [1, -1] {
                let mut normal = [0; 3];
                normal[axis] = sign;
                for slice in 0..N {
                    let neighbour = if sign > 0 { slice + 2 } else { slice };
                    let mut visible: [u16; N] =
                        std::array::from_fn(|row| solid[slice + 1][row] & !solid[neighbour][row]);
                    // Separate materials only for visible faces; occluded voxels cannot start a run.
                    let mut material_rows = [[0_u16; N]; crate::MAX_BLOCK as usize + 1];
                    let mut present = 0_u16;
                    for (row, bits) in visible.iter_mut().enumerate() {
                        let first = center + slice * STRIDE[axis] + row * STRIDE[v];
                        while *bits != 0 {
                            let column = bits.trailing_zeros() as usize;
                            let block = blocks[first + column * STRIDE[u]] as usize;
                            material_rows[block][row] |= 1 << column;
                            present |= 1 << block;
                            *bits &= *bits - 1;
                        }
                    }
                    while present != 0 {
                        let block = present.trailing_zeros() as usize;
                        present &= present - 1;
                        let rows = &mut material_rows[block];
                        for row in 0..N {
                            while rows[row] != 0 {
                                let column = rows[row].trailing_zeros() as usize;
                                let width = (rows[row] >> column).trailing_ones() as usize;
                                let run = (((1_u32 << width) - 1) << column) as u16;
                                let mut end = row + 1;
                                while end < N && rows[end] & run == run {
                                    end += 1;
                                }
                                for bits in &mut rows[row..end] {
                                    *bits &= !run;
                                }
                                let mut start = origin;
                                start[axis] += slice as i64 + i64::from(sign > 0);
                                start[u] += column as i64;
                                start[v] += row as i64;
                                let mut right = start;
                                right[u] += width as i64;
                                let mut top = start;
                                top[v] += (end - row) as i64;
                                let mut opposite = right;
                                opposite[v] = top[v];
                                let corners = if sign > 0 {
                                    [start, right, opposite, top]
                                } else {
                                    [start, top, opposite, right]
                                };
                                faces.push(Face {
                                    corners: corners.map(|p| p.map(|x| x as f32)),
                                    normal,
                                    block: block as u8,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
}
