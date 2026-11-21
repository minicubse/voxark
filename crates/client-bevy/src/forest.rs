use crate::terrain::Geometry;
use bevy::{
    asset::embedded_asset,
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
};
use primal_voxel::{ChunkPos, Face, MeshSnapshot};

pub type ForestMaterial = ExtendedMaterial<StandardMaterial, Grain>;
#[derive(Asset, AsBindGroup, TypePath, Clone, Debug, Default)]
pub struct Grain {}
impl MaterialExtension for Grain {
    fn fragment_shader() -> ShaderRef {
        "embedded://primal_client_bevy/forest.wesl".into()
    }
}
pub struct ForestPlugin;
impl Plugin for ForestPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "forest.wesl");
        app.add_plugins(MaterialPlugin::<ForestMaterial>::default());
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub palette: bool,
    pub variation: bool,
    pub cull: bool,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            palette: true,
            variation: true,
            cull: true,
        }
    }
}

pub fn color(f: &Face, y: f32, lit: bool) -> [f32; 4] {
    let (dark, light) = match f.block {
        1 => ([88, 124, 63], [161, 173, 84]),
        2 => ([103, 73, 59], [142, 105, 73]),
        3 => ([91, 103, 112], [149, 145, 136]),
        4 => ([150, 132, 98], [206, 182, 128]),
        5 => ([80, 57, 48], [128, 91, 67]),
        6 => ([42, 91, 76], [112, 150, 73]),
        _ => ([65, 154, 152], [112, 210, 184]),
    };
    let t = ((y - 12.) / 62.).clamp(0., 1.);
    let [r, g, b] =
        std::array::from_fn(|i| ((dark[i] as f32) * (1. - t) + (light[i] as f32) * t) as u8);
    let col = Color::srgb_u8(r, g, b).to_linear();
    let n = f.normal;
    let k = if lit {
        1.
    } else {
        0.83 + n[1] as f32 * 0.18 - n[0] as f32 * 0.09 + n[2] as f32 * 0.05
    };
    [col.red * k, col.green * k, col.blue * k, 1.]
}
pub fn prepare(snapshot: &MeshSnapshot, style: Style, lit: bool, seed: u64) -> (Geometry, usize) {
    let faces = snapshot.greedy_mesh_local();
    let (mut geom, _) = crate::terrain::prepare(&faces);
    geom.forest = true;
    if style.palette {
        let c = snapshot.chunk();
        let mut colors = Vec::with_capacity(faces.len() * 4);
        for f in &faces {
            for p in f.corners {
                colors.push(color(f, c[1] as f32 * 16. + p[1], lit));
            }
        }
        geom.mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    }
    if style.variation && style.palette {
        let c: ChunkPos = snapshot.chunk();
        let phase = [
            ((i64::from(c[0]) * 16 + (seed % 256) as i64).rem_euclid(256)) as f32,
            ((i64::from(c[2]) * 16 + ((seed >> 8) % 256) as i64).rem_euclid(256)) as f32,
        ];
        let mut uv = Vec::with_capacity(faces.len() * 4);
        let mut uv_b = Vec::with_capacity(faces.len() * 4);
        for f in &faces {
            for p in f.corners {
                uv.push([phase[0] + p[0], phase[1] + p[2]]);
                uv_b.push([c[1] as f32 * 16. + p[1], f.block as f32]);
            }
        }
        geom.mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
        geom.mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv_b);
        geom.bytes += faces.len() * 4 * 16;
    }
    let bytes = geom.bytes;
    (geom, bytes)
}
