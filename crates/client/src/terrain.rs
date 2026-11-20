use crate::buffers::Buffers;
use macroquad::miniquad::CullFace;
use macroquad::prelude::*;
#[cfg(test)]
use primal_voxel::Face;
use primal_voxel::{CHUNK, ChunkPos};
use std::collections::BTreeMap;

#[cfg(test)]
const FACES_PER_MESH: usize = 2048;
pub const FOG_COLOR: Color = Color::new(0.70, 0.79, 0.70, 1.0);
pub const VERTICAL_FOV: f32 = 65.0_f32.to_radians();

#[derive(Default)]
pub struct Terrain {
    chunks: BTreeMap<ChunkPos, Vec<Mesh>>,
    pub faces: usize,
}

impl Terrain {
    pub fn remove(&mut self, c: ChunkPos) {
        if let Some(old) = self.chunks.remove(&c) {
            self.faces -= old.iter().map(|m| m.indices.len() / 6).sum::<usize>();
        }
    }
    pub fn install(&mut self, c: ChunkPos, b: Buffers) {
        self.remove(c);
        if b.quads > 0 {
            self.faces += b.quads;
            self.chunks.insert(c, b.into_meshes());
        }
    }
    pub fn has_geometry(&self, c: ChunkPos) -> bool {
        self.chunks.contains_key(&c)
    }
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    pub fn draw(&self, eye: Vec3, front: Vec3, distance: f32, frustum: bool) -> usize {
        let mut visible = 0;
        let right = front.cross(Vec3::Y).normalize();
        let up = right.cross(front).normalize();
        let vertical_tangent = (VERTICAL_FOV * 0.5).tan();
        let horizontal_tangent = vertical_tangent * screen_width() / screen_height();
        for (chunk, meshes) in &self.chunks {
            let center = vec3(
                (chunk[0] as f32 + 0.5) * CHUNK as f32,
                (chunk[1] as f32 + 0.5) * CHUNK as f32,
                (chunk[2] as f32 + 0.5) * CHUNK as f32,
            );
            let offset = center - eye;
            let radius = (3.0_f32).sqrt() * CHUNK as f32 * 0.5;
            let in_view = if frustum {
                sphere_visible(
                    offset,
                    front,
                    right,
                    up,
                    horizontal_tangent,
                    vertical_tangent,
                    radius,
                    distance,
                )
            } else {
                offset.length_squared() <= (distance + radius).powi(2)
            };
            if !in_view {
                continue;
            }
            for mesh in meshes {
                draw_mesh(mesh);
            }
            visible += 1;
        }
        visible
    }
}

#[allow(clippy::too_many_arguments)]
fn sphere_visible(
    offset: Vec3,
    front: Vec3,
    right: Vec3,
    up: Vec3,
    horizontal_tangent: f32,
    vertical_tangent: f32,
    radius: f32,
    distance: f32,
) -> bool {
    let depth = offset.dot(front);
    offset.length_squared() <= (distance + radius).powi(2)
        && depth + radius >= 0.05
        && offset.dot(right).abs()
            <= depth * horizontal_tangent + radius * (1.0 + horizontal_tangent.powi(2)).sqrt()
        && offset.dot(up).abs()
            <= depth * vertical_tangent + radius * (1.0 + vertical_tangent.powi(2)).sqrt()
}

#[cfg(test)]
fn meshes(faces: &[Face]) -> Vec<Mesh> {
    Buffers::prepare(faces).0.into_meshes()
}

pub fn block_color(block: u8) -> Color {
    primal_voxel::block_def(block).map_or(WHITE, |block| {
        Color::from_rgba(block.rgb[0], block.rgb[1], block.rgb[2], 255)
    })
}

pub fn fog_material() -> Result<Material, macroquad::Error> {
    load_material(
        ShaderSource::Glsl {
            vertex: VERTEX_SHADER,
            fragment: FRAGMENT_SHADER,
        },
        MaterialParams {
            uniforms: vec![
                UniformDesc::new("CameraPosition", UniformType::Float3),
                UniformDesc::new("FogDistance", UniformType::Float1),
            ],
            pipeline_params: PipelineParams {
                depth_test: Comparison::LessOrEqual,
                depth_write: true,
                cull_face: CullFace::Nothing,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}

const VERTEX_SHADER: &str = r#"#version 100
attribute vec3 position;
attribute vec4 color0;
attribute vec4 normal;
uniform mat4 Model;
uniform mat4 Projection;
varying lowp vec4 color;
varying highp vec3 world;
varying lowp vec3 face_normal;
void main() {
    world = (Model * vec4(position, 1.0)).xyz;
    gl_Position = Projection * vec4(world, 1.0);
    color = color0 / 255.0;
    face_normal = normal.xyz;
}"#;

const FRAGMENT_SHADER: &str = r#"#version 100
precision mediump float;
varying lowp vec4 color;
varying highp vec3 world;
varying lowp vec3 face_normal;
uniform vec3 CameraPosition;
uniform float FogDistance;
void main() {
    float distance = length(world - CameraPosition);
    float fog = smoothstep(FogDistance * 0.40, FogDistance, distance);
    vec3 block = floor(world - face_normal * 0.001);
    float variation = 0.94 + fract(sin(dot(block, vec3(12.9898, 78.233, 37.719))) * 43758.5453) * 0.12;
    gl_FragColor = vec4(mix(color.rgb * variation, vec3(0.70, 0.79, 0.70), fog * 0.96), 1.0);
}"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frustum_keeps_intersecting_chunks_and_rejects_offscreen_chunks() {
        let visible =
            |offset| sphere_visible(offset, Vec3::Z, Vec3::X, Vec3::Y, 1.0, 1.0, 14.0, 100.0);
        assert!(visible(Vec3::new(0.0, 0.0, 40.0)));
        assert!(visible(Vec3::new(50.0, 0.0, 40.0)));
        assert!(!visible(Vec3::new(80.0, 0.0, 20.0)));
        assert!(!visible(Vec3::new(0.0, 0.0, -20.0)));
        assert!(!visible(Vec3::new(0.0, 0.0, 120.0)));
    }

    #[test]
    fn splits_meshes_without_losing_faces_or_overflowing_indices() {
        let face = Face {
            corners: [[0.0; 3]; 4],
            normal: [0, 1, 0],
            block: 1,
        };
        let faces = vec![face; FACES_PER_MESH * 2 + 1];
        let result = meshes(&faces);
        assert_eq!(result.len(), 3);
        assert_eq!(
            result
                .iter()
                .map(|mesh| mesh.indices.len() / 6)
                .sum::<usize>(),
            faces.len()
        );
        for mesh in result {
            assert!(mesh.vertices.len() <= FACES_PER_MESH * 4);
            assert!(
                mesh.indices
                    .iter()
                    .all(|index| usize::from(*index) < mesh.vertices.len())
            );
        }
    }
}
