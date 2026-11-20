use macroquad::prelude::{Mesh, Vec4, Vertex};
use primal_voxel::Face;

use crate::terrain::block_color;

// Macroquad indexes each batch with u16. The same limit was used by the original renderer.
const FACES_PER_MESH: usize = 2048;

struct Batch {
    vertices: Vec<Vertex>,
    indices: Vec<u16>,
}

/// Worker output contains owned CPU arrays only. Texture and draw state stay in Terrain.
pub struct Buffers {
    batches: Vec<Batch>,
    pub quads: usize,
}

impl Buffers {
    pub fn prepare(faces: &[Face]) -> (Self, usize) {
        let batches: Vec<_> = faces
            .chunks(FACES_PER_MESH)
            .map(|faces| {
                let mut vertices = Vec::with_capacity(faces.len() * 4);
                let mut indices = Vec::with_capacity(faces.len() * 6);
                for face in faces {
                    let base = vertices.len() as u16;
                    let normal = face.normal.map(|value| value as f32);
                    let shade = 0.72 + normal[1] * 0.20 - normal[0] * 0.08 + normal[2] * 0.05;
                    let mut color = block_color(face.block);
                    color.r *= shade;
                    color.g *= shade;
                    color.b *= shade;
                    for [x, y, z] in face.corners {
                        let mut vertex = Vertex::new(x, y, z, 0.0, 0.0, color);
                        vertex.normal = Vec4::new(normal[0], normal[1], normal[2], 0.0);
                        vertices.push(vertex);
                    }
                    indices.extend_from_slice(&[
                        base,
                        base + 1,
                        base + 2,
                        base,
                        base + 2,
                        base + 3,
                    ]);
                }
                Batch { vertices, indices }
            })
            .collect();
        let bytes = batches.capacity() * size_of::<Batch>()
            + batches
                .iter()
                .map(|batch| {
                    batch.vertices.capacity() * size_of::<Vertex>()
                        + batch.indices.capacity() * size_of::<u16>()
                })
                .sum::<usize>();
        (
            Self {
                batches,
                quads: faces.len(),
            },
            bytes,
        )
    }

    /// Moves existing allocations into Macroquad's representation on the main thread.
    pub fn into_meshes(self) -> Vec<Mesh> {
        self.batches
            .into_iter()
            .map(|batch| Mesh {
                vertices: batch.vertices,
                indices: batch.indices,
                texture: None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offthread_preparation_keeps_array_ownership_winding_and_normals() {
        let face = Face {
            corners: [[0., 1., 0.], [0., 1., 1.], [1., 1., 1.], [1., 1., 0.]],
            normal: [0, 1, 0],
            block: 3,
        };
        let (buffers, bytes) = std::thread::spawn(move || Buffers::prepare(&[face]))
            .join()
            .unwrap();
        let allocation = buffers.batches[0].vertices.as_ptr();
        let mesh = buffers.into_meshes().pop().unwrap();
        assert_eq!(mesh.vertices.as_ptr(), allocation);
        assert_eq!(mesh.indices, [0, 1, 2, 0, 2, 3]);
        assert_eq!(mesh.vertices[2].position.to_array(), [1., 1., 1.]);
        assert_eq!(mesh.vertices[2].normal.to_array(), [0., 1., 0., 0.]);
        assert!(bytes >= 4 * size_of::<Vertex>() + 6 * size_of::<u16>());
    }

    #[test]
    fn splits_meshes_without_losing_faces_or_overflowing_indices() {
        let face = Face {
            corners: [[0.0; 3]; 4],
            normal: [0, 1, 0],
            block: 1,
        };
        let faces = vec![face; FACES_PER_MESH * 2 + 1];
        let (buffers, _) = Buffers::prepare(&faces);
        assert_eq!(buffers.quads, faces.len());
        let result = buffers.into_meshes();
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
