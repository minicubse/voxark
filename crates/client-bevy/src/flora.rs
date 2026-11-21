//! Chunk-local botanical meshes. The far mesh keeps a deterministic subset of the same plants.
use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology,
};
use primal_voxel::{ChunkPos, Face};
#[derive(Default)]
pub struct Flora {
    pub near: Option<Mesh>,
    pub far: Option<Mesh>,
    pub supports: Vec<[u8; 3]>,
    pub far_count: usize,
    pub near_tri: usize,
    pub far_tri: usize,
    pub bytes: usize,
}
#[derive(Default)]
struct Batch {
    p: Vec<[f32; 3]>,
    n: Vec<[f32; 3]>,
    c: Vec<[f32; 4]>,
    idx: Vec<u32>,
}
impl Batch {
    fn leaf(&mut self, start: Vec3, tip: Vec3, side: Vec3, mid: f32, col: [u8; 3], base: f32) {
        let root = self.p.len() as u32;
        let joint = start.lerp(tip, mid);
        let normal = (tip - start).cross(side).normalize_or_zero();
        let normal = (Vec3::Y * 0.7 + normal * 0.3).normalize_or_zero();
        let c = Color::srgb_u8(col[0], col[1], col[2]).to_linear();
        for (p, light) in [
            (start, base),
            (joint - side, 0.93),
            (tip, 1.13),
            (joint + side, 1.),
        ] {
            self.p.push(p.to_array());
            self.n.push(normal.to_array());
            self.c
                .push([c.red * light, c.green * light, c.blue * light, 1.]);
        }
        self.idx
            .extend([root, root + 1, root + 2, root, root + 2, root + 3]);
    }
    fn finish(self) -> (Option<Mesh>, usize, usize) {
        let tris = self.idx.len() / 3;
        let bytes = self.p.len() * 40 + self.idx.len() * 4;
        if tris == 0 {
            return (None, 0, 0);
        }
        (
            Some(
                Mesh::new(
                    PrimitiveTopology::TriangleList,
                    RenderAssetUsages::RENDER_WORLD,
                )
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.p)
                .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.n)
                .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.c)
                .with_inserted_indices(Indices::U32(self.idx)),
            ),
            tris,
            bytes,
        )
    }
}
fn hash(pos: [i64; 3], seed: u64) -> u64 {
    let mut h = seed;
    for i in pos {
        h = (h ^ (i as u64).wrapping_mul(0x9e3779b97f4a7c15))
            .rotate_left(27)
            .wrapping_mul(0xbf58476d1ce4e5b9);
    }
    h ^= h >> 30;
    h = h.wrapping_mul(0x94d049bb133111eb);
    h ^ (h >> 31)
}
fn paint(b: &mut Batch, center: Vec3, r: u64, kind: u32, sparse: bool) {
    let angle = (r as u16 as f32) * 0.000096;
    let size = 0.68 + ((r >> 24) & 255) as f32 / 380.;
    let stalk = |a: f32| Vec3::new(a.cos(), 0., a.sin());
    let y = Vec3::Y * size;
    let fern = [61, 126, 74];
    let grass = if r & 4 == 0 {
        [151, 155, 66]
    } else {
        [105, 150, 75]
    };
    match kind {
        0 | 1 => {
            // Grasses and folded sedge blades.
            let count = if sparse { 3 } else { 6 };
            for j in 0..count {
                let d = stalk(angle + j as f32 * 2.39996);
                let foot = center + d * size * 0.12;
                b.leaf(
                    foot,
                    foot + d * (0.18 + 0.06 * (j % 3) as f32) * size + y * (0.35 + 0.06 * j as f32),
                    d.cross(y) * 0.045,
                    0.27,
                    grass,
                    0.55,
                );
            }
        }
        2 => {
            // Arching fern fronds and opposed leaflets.
            for j in 0..if sparse { 3 } else { 5 } {
                let d = stalk(angle + j as f32 * 2.39996);
                let side = d.cross(y);
                let crown = center + d * size * 0.65 + y * 0.48;
                b.leaf(center, crown, side * 0.026, 0.38, fern, 0.62);
                for k in 1..=if sparse { 2 } else { 4 } {
                    let t = k as f32 / 5.;
                    let stem =
                        center.lerp(crown, t) + y * (0.10 * (t * std::f32::consts::PI).sin());
                    for sign in [-1., 1.] {
                        let tip =
                            stem + side * sign * 0.20 * (1.15 - t) + d * size * 0.16 + y * 0.04;
                        b.leaf(stem, tip, d * size * 0.042, 0.45, [76, 145, 81], 0.66);
                    }
                }
            }
        }
        3 | 4 => {
            // Bells and umbels with contrasting throats.
            let d = stalk(angle);
            let head = center + y * (if kind == 3 { 0.76 } else { 0.53 }) + d * size * 0.07;
            b.leaf(center, head, d.cross(y) * 0.021, 0.5, [86, 112, 58], 0.68);
            for j in 0..if sparse { 3 } else { 5 } {
                let v = stalk(angle + j as f32 * 1.25664);
                let petal = if kind == 3 {
                    [166, 128, 197]
                } else {
                    [241, 192, 87]
                };
                b.leaf(
                    head,
                    head + v * size * 0.115 + y * 0.055,
                    v.cross(y) * 0.053,
                    0.68,
                    petal,
                    0.9,
                );
            }
            b.leaf(
                head,
                head + y * 0.085,
                d.cross(y) * 0.037,
                0.5,
                [249, 206, 116],
                0.92,
            );
            b.leaf(
                center + y * 0.12,
                center + y * 0.35 - d * size * 0.18,
                d.cross(y) * 0.063,
                0.45,
                fern,
                0.58,
            );
        }
        5 => {
            // A ring of facets forms each ochre mushroom cap.
            let d = stalk(angle);
            let top = center + y * 0.27;
            b.leaf(center, top, d.cross(y) * 0.043, 0.5, [201, 181, 145], 0.75);
            for j in 0..6 {
                let v = stalk(angle + j as f32 * std::f32::consts::TAU / 6.);
                b.leaf(
                    top + y * 0.065,
                    top + v * size * 0.15 - y * 0.035,
                    v.cross(y) * 0.065,
                    0.72,
                    [192, 105, 59],
                    0.83,
                );
            }
        }
        _ => {
            let d = stalk(angle);
            b.leaf(
                center - d * size * 0.28 + y * 0.04,
                center + d * size * 0.38 + y * 0.09,
                d.cross(y) * 0.055,
                0.45,
                [130, 98, 68],
                0.74,
            );
            b.leaf(
                center + y * 0.07,
                center + d.cross(y) * 0.22 + y * 0.12,
                d * size * 0.033,
                0.4,
                [133, 107, 69],
                0.8,
            );
        }
    }
}
pub fn build(faces: &[Face], chunk: ChunkPos, seed: u64, density: f32) -> Flora {
    let mut result = Flora::default();
    if density <= 0. {
        return result;
    }
    let mut near = Batch::default();
    let mut far = Batch::default();
    for f in faces
        .iter()
        .filter(|f| f.normal == [0, 1, 0] && matches!(f.block, 1 | 2 | 4))
    {
        let a = f.corners[0];
        let c = f.corners[2];
        let y = a[1] as i32 - 1;
        for x in (a[0].min(c[0]) as i32)..(a[0].max(c[0]) as i32) {
            for z in (a[2].min(c[2]) as i32)..(a[2].max(c[2]) as i32) {
                let pos = [x, y, z];
                let abs = std::array::from_fn(|i| i64::from(chunk[i]) * 16 + i64::from(pos[i]));
                let r = hash(abs, seed);
                let patch = hash([abs[0].div_euclid(5), 0, abs[2].div_euclid(5)], seed);
                let rate = density
                    * (if f.block == 1 { 1. } else { 0.29 })
                    * (0.35 + (patch & 255) as f32 / 340.);
                if ((r >> 40) & 65535) as f32 / 65536. >= rate {
                    continue;
                }
                let cx = x as f32 + 0.22 + ((r >> 16) & 255) as f32 / 455.;
                let cz = z as f32 + 0.22 + ((r >> 32) & 255) as f32 / 455.;
                let center = Vec3::new(cx, y as f32 + 1.004, cz);
                let kind = if f.block == 2 {
                    if r & 3 == 0 { 5 } else { 6 }
                } else if f.block == 4 {
                    1
                } else {
                    match (patch.wrapping_add(r & 3)) % 17 {
                        0 | 1 => 2,
                        2 => 3,
                        3 => 4,
                        4 => 5,
                        5 => 6,
                        _ => 0,
                    }
                };
                paint(&mut near, center, r, kind, false);
                result.supports.push(pos.map(|v| v as u8));
                if r & 3 == 0 {
                    paint(&mut far, center, r, kind, true);
                    result.far_count += 1;
                }
            }
        }
    }
    let (n, nt, nb) = near.finish();
    let (f, ft, fb) = far.finish();
    result.near = n;
    result.far = f;
    result.near_tri = nt;
    result.far_tri = ft;
    result.bytes = nb + fb;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn support_controls_all_lods() {
        let mut f = Face {
            corners: [[0., 1., 0.], [0., 1., 16.], [16., 1., 16.], [16., 1., 0.]],
            normal: [0, 1, 0],
            block: 1,
        };
        let a = build(&[f], [0, 0, 0], 42, 1.);
        let b = build(&[f], [0, 0, 0], 42, 1.);
        assert_eq!(a.supports, b.supports);
        assert!(a.supports.len() > 30 && a.far_count < a.supports.len() && a.far_tri < a.near_tri);
        f.block = 3;
        assert!(build(&[f], [0, 0, 0], 42, 1.).supports.is_empty());
        f.block = 1;
        f.normal = [0, -1, 0];
        assert_eq!(build(&[f], [0, 0, 0], 42, 1.).near_tri, 0);
    }
}
