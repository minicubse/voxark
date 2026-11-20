use super::{CHUNK, ChunkPos, Pos, VOLUME, Voxels, chunk_at, local_index, stream_chunk};

/// Immutable generator result. A worker can construct it without borrowing a world.
pub struct GeneratedChunk {
    pub(super) seed: u64,
    pub(super) chunk: ChunkPos,
    pub(super) cells: Voxels,
}

impl GeneratedChunk {
    pub fn get(&self, position: Pos) -> Option<u8> {
        (chunk_at(position) == self.chunk).then(|| self.cells.get(local_index(position)))
    }
}

struct Builder {
    origin: [i64; 3],
    cells: Box<[u8; VOLUME]>,
}

impl Builder {
    fn put(&mut self, x: i64, y: i64, z: i64, block: u8) {
        let p = [x - self.origin[0], y - self.origin[1], z - self.origin[2]];
        if p.iter().all(|v| (0..16).contains(v)) {
            self.cells[((p[1] * 16 + p[2]) * 16 + p[0]) as usize] = block;
        }
    }

    fn intersects(&self, min: [i64; 3], max: [i64; 3]) -> bool {
        (0..3).all(|axis| self.origin[axis] <= max[axis] && self.origin[axis] + 15 >= min[axis])
    }

    fn trees(&mut self, seed: u64) {
        let [x0, y0, z0] = self.origin;
        // Anchors jitter up to three blocks on their lattice, crowns extend by
        // four. Visit every candidate whose geometry can cross this chunk.
        for gz in (z0 - 7).div_euclid(13)..=(z0 + 15 + 7).div_euclid(13) {
            for gx in (x0 - 7).div_euclid(13)..=(x0 + 15 + 7).div_euclid(13) {
                let random = hash(seed, gx, gz);
                let x = gx * 13 + (random % 7) as i64 - 3;
                let z = gz * 13 + ((random >> 8) % 7) as i64 - 3;
                if x + 4 < x0
                    || x - 4 > x0 + 15
                    || z + 4 < z0
                    || z - 4 > z0 + 15
                    || random.is_multiple_of(4)
                {
                    continue;
                }
                let outside = distance_to_valley(x, z);
                if outside == 0 {
                    if gx.abs() > 4
                        || gz.abs() > 4
                        || x.abs() < 10
                        || !(-59..59).contains(&x)
                        || !(-59..59).contains(&z)
                    {
                        continue;
                    }
                } else if outside <= 4 {
                    // New crowns must never introduce leaves into the v1 volume.
                    continue;
                }
                let ground = height(seed, x, z);
                let suitable = if outside == 0 { 9..=26 } else { 14..=65 };
                let top = ground + 7 + ((random >> 16) % 3) as i64;
                if !suitable.contains(&ground) || y0 > top + 2 || y0 + 15 <= ground {
                    continue;
                }
                for y in ground + 1..=top {
                    self.put(x, y, z, 5);
                }
                for dy in -1_i64..=2 {
                    for dz in -4_i64..=4 {
                        for dx in -4_i64..=4 {
                            if dx.abs() + dz.abs() <= 5 - dy.max(0)
                                && (dx != 0 || dz != 0 || dy > 0)
                            {
                                self.put(x + dx, top + dy, z + dz, 6);
                            }
                        }
                    }
                }
            }
        }
    }

    fn landmarks(&mut self, seed: u64) {
        if self.intersects([-27, 12, 34], [-12, 31, 37]) {
            for x in -27_i64..=-12 {
                let distance = (x + 20).abs();
                for z in 34..=37 {
                    for y in 12..=31 {
                        if (distance >= 5 && y < 30) || ((28..=31).contains(&y) && distance <= 8) {
                            self.put(x, y, z, 3);
                        }
                    }
                }
            }
        }
        for (cx, cz) in [(-15, 24), (23, 38), (-38, -24)] {
            if !self.intersects([cx - 2, 0, cz - 2], [cx + 2, 55, cz + 2]) {
                continue;
            }
            let base = height(seed, cx, cz) + 1;
            for dz in -2_i64..=2 {
                for dx in -2_i64..=2 {
                    for dy in 0..(8 - (dx.abs() + dz.abs()) * 2) {
                        self.put(cx + dx, base + dy, cz + dz, 7);
                    }
                }
            }
        }
    }
}

/// Generate exactly one 16³ region. Work and allocation are independent of the
/// distance from the origin and of the number of chunks already explored.
pub fn generate_chunk(seed: u64, chunk: ChunkPos) -> Option<GeneratedChunk> {
    if !stream_chunk(chunk) {
        return None;
    }
    let origin = chunk.map(|v| i64::from(v) * i64::from(CHUNK));
    let [x0, y0, z0] = origin;
    let in_valley = (-64..64).contains(&x0) && (-64..64).contains(&z0);
    if y0 >= 96 || (in_valley && y0 >= 64) {
        return Some(GeneratedChunk {
            seed,
            chunk,
            cells: Voxels::Uniform(0),
        });
    }
    let mut builder = Builder {
        origin,
        cells: Box::new([0; VOLUME]),
    };
    for z in 0..16 {
        for x in 0..16 {
            let surface = height(seed, x0 + x, z0 + z);
            let top = (surface - y0).min(15);
            for y in 0..=top {
                let depth = surface - (y0 + y);
                builder.cells[((y * 16 + z) * 16 + x) as usize] = match depth {
                    4.. => 3,
                    1..=3 => 2,
                    _ if surface <= 8 => 4,
                    _ => 1,
                };
            }
        }
    }
    builder.trees(seed);
    if in_valley {
        builder.landmarks(seed);
    }
    let first = builder.cells[0];
    let cells = if builder.cells.iter().all(|&b| b == first) {
        Voxels::Uniform(first)
    } else {
        Voxels::Dense(builder.cells)
    };
    Some(GeneratedChunk { seed, chunk, cells })
}

fn distance_to_valley(x: i64, z: i64) -> i64 {
    (-64 - x).max(x - 63).max(-64 - z).max(z - 63).max(0)
}

fn height(seed: u64, x: i64, z: i64) -> i64 {
    let rolling = noise(seed, x, z, 16, 5) + noise(seed ^ 517, x, z, 7, 2);
    let left_peak = (29 - ((x + 43).abs() + (z - 43).abs()) * 2 / 3).max(0);
    let right_peak = (32 - ((x - 43).abs() + (z - 44).abs()) * 2 / 3).max(0);
    let valley_wall = ((x.abs() - 23).max(0) * 2 / 5).min(15);
    let historic = (8 + rolling + valley_wall + left_peak.max(right_peak)).min(47);
    let blend = distance_to_valley(x, z).min(64);
    if blend == 0 {
        return historic;
    }
    let hills = 8
        + rolling
        + noise(seed ^ 0x1347_43ac, x, z, 96, 43)
        + noise(seed ^ 0x987f_6531, x, z, 37, 20);
    (historic * (64 - blend) + hills * blend) / 64
}

fn hash(seed: u64, x: i64, z: i64) -> u64 {
    let mut value = seed
        ^ (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (z as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn noise(seed: u64, x: i64, z: i64, scale: i64, amplitude: i64) -> i64 {
    let gx = x.div_euclid(scale);
    let gz = z.div_euclid(scale);
    let tx = x.rem_euclid(scale);
    let tz = z.rem_euclid(scale);
    let sample = |dx, dz| (hash(seed, gx + dx, gz + dz) % 1024) as i64;
    let top = sample(0, 0) * (scale - tx) + sample(1, 0) * tx;
    let bottom = sample(0, 1) * (scale - tx) + sample(1, 1) * tx;
    ((top * (scale - tz) + bottom * tz) * amplitude) / (scale * scale * 1024)
}
