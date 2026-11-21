use super::{VOLUME, Voxels, generation::hash};

const SIZE: i64 = crate::CHUNK as i64;
const HALO: usize = SIZE as usize + 2;
const AIR: u8 = 0;
const GRASS: u8 = 1;
const EARTH: u8 = 2;
const STONE: u8 = 3;
const WOOD: u8 = 5;
const LEAVES: u8 = 6;
const DIRECTIONS: [[i64; 2]; 8] = [
    [1, 0],
    [1, 1],
    [0, 1],
    [-1, 1],
    [-1, 0],
    [-1, -1],
    [0, -1],
    [1, -1],
];

pub(super) fn height(seed: u64, x: i64, z: i64) -> i64 {
    let blend = (meadow_distance(x, z) - 196).clamp(0, 1024);
    if blend == 0 {
        return 24 + field(seed ^ 0x7a5e_d692, x, z, 29) / 512;
    }
    let hills = 18
        + field(seed ^ 0xb67e_2e18, x, z, 94) * 24 / 1024
        + field(seed ^ 0xd74a_bc65, x, z, 37) * 12 / 1024
        + field(seed ^ 0x4fae_8c30, x, z, 12) * 5 / 1024;
    let ridge = field(seed ^ 0xab58_f227, x + z / 3, z - x / 5, 118);
    let shelf = (ridge - 460).clamp(0, 90) * 14 / 90;
    // The sheltered meadow opens towards a wooded bench and its exposed step.
    let step = if (45..102).contains(&x) && z.abs() < 80 {
        let edge = 44 + ((z + 10).abs() / 4).min(28) + field(seed ^ 0x0587_1d19, x, z, 29) / 120;
        let fade = (102 - x).min(80 - z.abs()).min(30);
        (x - edge).clamp(0, 7) * 3 * fade / 30
    } else {
        0
    };
    if blend == 1024 {
        return hills + shelf + step;
    }
    let meadow = 24 + field(seed ^ 0x7a5e_d692, x, z, 29) / 512;
    (meadow * (1024 - blend) + (hills + shelf + step) * blend) / 1024
}

fn meadow_distance(x: i64, z: i64) -> i64 {
    let x = x.clamp(-48, 48);
    let z = z.clamp(-48, 48);
    x * x + z * z * 3 / 4
}

/// Smooth integer value noise. All samples come from global coordinates, so
/// the slope and ecology agree on both sides of horizontal chunk boundaries.
fn field(seed: u64, x: i64, z: i64, scale: i64) -> i64 {
    let gx = x.div_euclid(scale);
    let gz = z.div_euclid(scale);
    let weight = |coordinate: i64| {
        let t = coordinate.rem_euclid(scale) * 1024 / scale;
        t * t * (3072 - 2 * t) / (1024 * 1024)
    };
    let tx = weight(x);
    let tz = weight(z);
    let sample = |dx, dz| (hash(seed, gx + dx, gz + dz) & 1023) as i64;
    let top = sample(0, 0) * (1024 - tx) + sample(1, 0) * tx;
    let bottom = sample(0, 1) * (1024 - tx) + sample(1, 1) * tx;
    (top * (1024 - tz) + bottom * tz) / (1024 * 1024)
}

pub(super) fn generate(seed: u64, origin: [i64; 3]) -> Voxels {
    let [x0, y0, z0] = origin;
    let heights: [[i64; HALO]; HALO] = std::array::from_fn(|z| {
        std::array::from_fn(|x| height(seed, x0 + x as i64 - 1, z0 + z as i64 - 1))
    });
    let lowest = heights.iter().flatten().copied().min().unwrap();
    if y0 + SIZE + 3 <= lowest {
        return Voxels::Uniform(STONE);
    }

    let mut chunk = Chunk {
        origin,
        cells: Box::new([AIR; VOLUME]),
    };
    for z in 1..HALO - 1 {
        for x in 1..HALO - 1 {
            let ground = heights[z][x];
            let low_neighbor = heights[z][x - 1]
                .min(heights[z][x + 1])
                .min(heights[z - 1][x])
                .min(heights[z + 1][x]);
            let exposed = ground - low_neighbor >= 2;
            for y in y0..=ground.min(y0 + SIZE - 1) {
                let material = match (ground - y, exposed) {
                    (_, true) | (4.., false) => STONE,
                    (1..=3, false) => EARTH,
                    _ => GRASS,
                };
                chunk.cells[index(x as i64 - 1, y - y0, z as i64 - 1)] = material;
            }
        }
    }
    chunk.scatter(seed, Feature::Ground);
    chunk.scatter(seed, Feature::Tree);
    let first = chunk.cells[0];
    if chunk.cells.iter().all(|&block| block == first) {
        Voxels::Uniform(first)
    } else {
        Voxels::Dense(chunk.cells)
    }
}

fn index(x: i64, y: i64, z: i64) -> usize {
    ((y * SIZE + z) * SIZE + x) as usize
}

struct Chunk {
    origin: [i64; 3],
    cells: Box<[u8; VOLUME]>,
}

enum Feature {
    Ground,
    Tree,
}

impl Chunk {
    fn place(&mut self, [x, y, z]: [i64; 3], material: u8) {
        let [x0, y0, z0] = self.origin;
        let [dx, dy, dz] = [x - x0, y - y0, z - z0];
        if [dx, dy, dz].into_iter().all(|d| (0..SIZE).contains(&d)) {
            let cell = &mut self.cells[index(dx, dy, dz)];
            if *cell == AIR || (*cell == LEAVES && material == WOOD) {
                *cell = material;
            }
        }
    }

    fn scatter(&mut self, seed: u64, feature: Feature) {
        let [x0, y0, z0] = self.origin;
        let (spacing, reach, feature_seed) = match feature {
            Feature::Tree => (9, 9, seed ^ 0x7d40_162c),
            Feature::Ground => (13, 7, seed ^ 0x2cab_04e9),
        };
        let jitter = 3;
        let padding = reach + jitter;
        let jitter_width = (jitter * 2 + 1) as u64;
        // Each chunk visits every anchor whose geometry could reach it, in
        // the same global order. Roots and crowns can extend across any face.
        for gz in (z0 - padding).div_euclid(spacing)..=(z0 + SIZE - 1 + padding).div_euclid(spacing)
        {
            for gx in
                (x0 - padding).div_euclid(spacing)..=(x0 + SIZE - 1 + padding).div_euclid(spacing)
            {
                let key = hash(feature_seed, gx, gz);
                let x = gx * spacing + (key % jitter_width) as i64 - jitter;
                let z = gz * spacing + ((key >> 8) % jitter_width) as i64 - jitter;
                if x + reach < x0
                    || x - reach >= x0 + SIZE
                    || z + reach < z0
                    || z - reach >= z0 + SIZE
                    || i32::try_from(x).is_err()
                    || i32::try_from(z).is_err()
                {
                    continue;
                }
                let distance = meadow_distance(x, z);
                match feature {
                    Feature::Tree if distance > 22 * 22 && (key >> 17) % 16 >= 3 => {
                        let ground = height(seed, x, z);
                        if y0 + SIZE > ground - 2 && y0 <= ground + 26 {
                            self.tree(seed, [x, ground, z], key);
                        }
                    }
                    Feature::Ground if distance > 17 * 17 && (key >> 17) % 16 >= 6 => {
                        let ground = height(seed, x, z);
                        if y0 + SIZE > ground - 2 && y0 <= ground + 5 {
                            self.forest_floor(seed, [x, ground, z], key);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn tree(&mut self, seed: u64, [x, ground, z]: [i64; 3], key: u64) {
        let age = match (key >> 24) % 16 {
            0..=2 => 0,
            14..=15 => 2,
            _ => 1,
        };
        let variation = ((key >> 29) % 4) as i64;
        let grove = field(seed ^ 0x2cfa_4b89, x, z, 53) / 28 - 18;
        let species = ((key >> 34) % 100) as i64 + grove;
        let [dx, dz] = DIRECTIONS[((key >> 41) % 8) as usize];
        let width = i64::from(age == 2 && species < 77);
        let trunk = [10, 15, 21][age] + variation;
        let trunk = match species {
            ..42 => trunk,
            42..77 => trunk - 3,
            _ => trunk - 1,
        };
        let lean = i64::from(species >= 42 && age != 0);
        let elbow = trunk / 2;
        for rise in (self.origin[1] - ground).max(1)..=trunk.min(self.origin[1] + SIZE - 1 - ground)
        {
            let bend = i64::from(rise >= elbow) * lean;
            for ox in 0..=width {
                for oz in 0..=width {
                    self.place(
                        [x + dx * bend + ox, ground + rise, z + dz * bend + oz],
                        WOOD,
                    );
                }
            }
            if bend == 1 && rise == elbow {
                self.place([x, ground + rise, z], WOOD);
                self.place([x + dx, ground + rise, z], WOOD);
            }
        }
        if age > 0 && species < 77 && self.origin[1] <= ground + 4 {
            self.roots(seed, [x, ground, z], age as i64 + 1, (key >> 45) as usize);
        }
        let crown = [x + dx * lean, ground + trunk, z + dz * lean];
        if species < 42 {
            self.conifer(crown, trunk, age, key);
        } else if species < 77 {
            self.broadleaf(crown, age, key);
        } else {
            self.birch(crown, age, key);
        }
    }

    fn roots(&mut self, seed: u64, [x, ground, z]: [i64; 3], length: i64, angle: usize) {
        for offset in [0, 2, 5] {
            let [dx, dz] = DIRECTIONS[angle.wrapping_add(offset) % 8];
            let mut previous = ground;
            for step in 1..=length {
                let [rx, rz] = [x + dx * step, z + dz * step];
                let surface = height(seed, rx, rz);
                if (surface - previous).abs() > 1 {
                    break;
                }
                self.place([rx, previous.max(surface) + 1, rz], WOOD);
                if previous > surface {
                    self.place([rx, surface + 1, rz], WOOD);
                }
                if dx != 0 && dz != 0 {
                    self.place([rx, previous + 1, rz - dz], WOOD);
                }
                previous = surface;
            }
        }
    }

    fn conifer(&mut self, [x, y, z]: [i64; 3], trunk: i64, age: usize, key: u64) {
        let depth = trunk * 3 / 4;
        let radius = [3, 5, 6][age];
        let base = y - depth;
        for layer in base.max(self.origin[1])..=(y + 2).min(self.origin[1] + SIZE - 1) {
            let rise = layer - base;
            let tier = rise % [3, 4, 5][age];
            let minimum = i64::from(layer <= y);
            let width = ((depth + 2 - rise) * radius / depth - tier / 2).clamp(minimum, radius);
            if width == 0 {
                self.place([x, layer, z], LEAVES);
            } else {
                self.leaf_ball([x, layer, z], [width, 1, width], key, true);
            }
        }
    }

    fn broadleaf(&mut self, [x, y, z]: [i64; 3], age: usize, key: u64) {
        let width = [2, 3, 4][age];
        self.leaf_ball([x, y + 1, z], [width, 2, width], key, false);
        for offset in [0, 3, 5] {
            let [dx, dz] = DIRECTIONS[((key >> 45) as usize).wrapping_add(offset) % 8];
            let branch = width - i64::from(dx != 0 && dz != 0);
            let drop = 3 + ((key >> (51 + offset)) & 1) as i64;
            for distance in 1..=branch {
                let by = y - drop + distance / 2;
                let [bx, bz] = [x + dx * distance, z + dz * distance];
                self.place([bx, by, bz - dz], WOOD);
                self.place([bx, by, bz], WOOD);
                self.place([bx - dx, by, bz - dz], WOOD);
            }
            self.leaf_ball(
                [x + dx * branch, y - drop + branch / 2 + 1, z + dz * branch],
                [width, 2 + i64::from(age == 2), width],
                key.rotate_left(offset as u32),
                false,
            );
        }
    }

    fn birch(&mut self, [x, y, z]: [i64; 3], age: usize, key: u64) {
        let [dx, dz] = DIRECTIONS[((key >> 50) % 8) as usize];
        let radius = [2, 2, 3][age];
        self.leaf_ball([x, y, z], [radius, 3, radius], key, false);
        for side in [-1, 1] {
            let by = y - 4 + side;
            let [bx, bz] = [x + dx * side, z + dz * side];
            self.place([bx, by, z], WOOD);
            self.place([bx, by, bz], WOOD);
            self.leaf_ball(
                [bx, by + 1, bz],
                [radius, 2, radius],
                key.rotate_left(7),
                false,
            );
        }
    }

    fn leaf_ball(
        &mut self,
        [x, y, z]: [i64; 3],
        [rx, ry, rz]: [i64; 3],
        key: u64,
        single_layer: bool,
    ) {
        let [x0, y0, z0] = self.origin;
        let [xx, yy, zz] = [rx * rx, ry * ry, rz * rz];
        let edge = xx * yy * zz;
        let height = if single_layer { 0 } else { ry };
        for by in (y - height).max(y0)..=(y + height).min(y0 + SIZE - 1) {
            for bz in (z - rz).max(z0)..=(z + rz).min(z0 + SIZE - 1) {
                for bx in (x - rx).max(x0)..=(x + rx).min(x0 + SIZE - 1) {
                    let distance = (bx - x).pow(2) * yy * zz
                        + (by - y).pow(2) * xx * zz
                        + (bz - z).pow(2) * xx * yy;
                    if distance <= edge
                        && (distance * 5 < edge * 4 || hash(key ^ by as u64, bx, bz) % 5 != 0)
                    {
                        self.place([bx, by, bz], LEAVES);
                    }
                }
            }
        }
    }

    fn forest_floor(&mut self, seed: u64, [x, ground, z]: [i64; 3], key: u64) {
        if (key >> 24) % 5 < 2 {
            let [dx, dz] = if key & 1 == 0 { [1, 0] } else { [0, 1] };
            let length = 5 + ((key >> 30) % 4) as i64;
            let far = height(seed, x + dx * (length - 1), z + dz * (length - 1));
            if (ground - far).abs() > 1 {
                return;
            }
            for step in 0..length {
                let [bx, bz] = [x + dx * step, z + dz * step];
                let below = height(seed, bx, bz);
                if (below - ground).abs() > 1 {
                    break;
                }
                if below < ground {
                    self.place([bx, below + 1, bz], WOOD);
                }
                self.place([bx, below.max(ground) + 1, bz], WOOD);
                if step < 2 {
                    self.place([bx, below.max(ground) + 2, bz], WOOD);
                }
            }
        } else {
            let radius = 1 + ((key >> 28) % 3) as i64;
            for dz in -radius..=radius {
                for dx in -radius..=radius {
                    let distance = dx * dx + dz * dz;
                    if distance > radius * radius + 1 {
                        continue;
                    }
                    let [bx, bz] = [x + dx, z + dz];
                    let below = height(seed, bx, bz);
                    if below < ground - 2 {
                        continue;
                    }
                    let cap = ground + 1 + (radius * radius - distance).max(0) / radius;
                    for y in (ground - 1).max(below + 1)..=cap {
                        let moss = y == cap && hash(key, bx, bz) % 5 < 3;
                        self.place([bx, y, bz], if moss { LEAVES } else { STONE });
                    }
                }
            }
        }
    }
}
