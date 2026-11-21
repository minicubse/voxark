use bevy::{
    asset::RenderAssetUsages, camera::visibility::NoFrustumCulling, math::DVec3, mesh::Indices,
    prelude::*, render::render_resource::PrimitiveTopology,
};
use primal_voxel::{ChunkPos, Face, block_def};
use std::collections::{HashMap, HashSet, VecDeque};

pub struct Geometry {
    pub forest: bool,
    pub flora: crate::flora::Flora,
    pub build_us: f64,
    pub mesh: Mesh,
    pub quads: usize,
    pub bytes: usize,
}
pub fn prepare(faces: &[Face]) -> (Geometry, usize) {
    let mut positions = Vec::with_capacity(faces.len() * 4);
    let mut normals = Vec::with_capacity(faces.len() * 4);
    let mut colors = Vec::with_capacity(faces.len() * 4);
    let mut indices = Vec::with_capacity(faces.len() * 6);
    for face in faces {
        let n = face.normal.map(|v| v as f32);
        let shade = 0.72 + n[1] * 0.20 - n[0] * 0.08 + n[2] * 0.05;
        let [r, g, b] = block_def(face.block)
            .expect("catalog material")
            .rgb
            .map(|v| (v as f32 * shade).clamp(0., 255.) as u8);
        let linear = Color::srgb_u8(r, g, b).to_linear();
        let base = positions.len() as u32;
        positions.extend_from_slice(&face.corners);
        normals.extend_from_slice(&[n; 4]);
        colors.extend_from_slice(&[[linear.red, linear.green, linear.blue, 1.]; 4]);
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let bytes = faces.len() * (4 * 40 + 6 * 4);
    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices));
    (
        Geometry {
            forest: false,
            flora: Default::default(),
            build_us: 0.,
            mesh,
            quads: faces.len(),
            bytes,
        },
        bytes,
    )
}
pub fn offset(chunk: ChunkPos, origin: ChunkPos) -> Vec3 {
    Vec3::from_array(std::array::from_fn(|i| {
        ((i64::from(chunk[i]) - i64::from(origin[i])) * 16) as f32
    }))
}
pub fn origin_for(position: DVec3) -> ChunkPos {
    (position / 16.).floor().as_ivec3().to_array()
}
struct Tile {
    forest: bool,
    flora: [Option<Entity>; 2],
    flora_tri: [usize; 2],
    plants: [usize; 2],
    lod: usize,
    supports: Vec<[u8; 3]>,
    entity: Entity,
    quads: usize,
    visible: bool,
}
#[derive(Resource, Default)]
pub struct Terrain {
    tiles: HashMap<ChunkPos, Tile>,
    pending: HashMap<ChunkPos, Geometry>,
    order: VecDeque<ChunkPos>,
    retired: HashSet<ChunkPos>,
    pub origin: ChunkPos,
    pub rebases: usize,
    pub peak: usize,
    pub uploads: usize,
    pub bytes: usize,
    pub publish_us: f64,
    pub visible: usize,
    pub quads: usize,
    pub removed: usize,
    pub flora_tri: usize,
    pub plants: usize,
    pub build_us: f64,
    pub built: usize,
    pub stream_us: f64,
    pub stream_ticks: usize,
}
impl Terrain {
    pub fn install(&mut self, c: ChunkPos, g: Geometry) {
        self.build_us += g.build_us;
        self.built += 1;
        self.retired.remove(&c);
        if self.pending.insert(c, g).is_none() {
            self.order.push_back(c);
        }
    }
    pub fn remove(&mut self, c: ChunkPos) {
        if self.pending.remove(&c).is_some() {
            self.order.retain(|k| *k != c);
        }
        if self.tiles.contains_key(&c) {
            self.retired.insert(c);
        }
    }
    pub fn clear(&mut self) {
        self.pending.clear();
        self.order.clear();
        self.retired.extend(self.tiles.keys().copied());
    }
    pub fn has_geometry(&self, c: ChunkPos) -> bool {
        self.tiles.contains_key(&c) && !self.retired.contains(&c)
    }
    pub fn has_plant(&self, pos: [i32; 3]) -> bool {
        let c = primal_voxel::chunk_at(pos);
        self.tiles
            .get(&c)
            .is_some_and(|t| t.supports.contains(&pos.map(|v| v.rem_euclid(16) as u8)))
    }
    pub fn idle(&self) -> bool {
        self.pending.is_empty() && self.retired.is_empty()
    }
    pub fn len(&self) -> usize {
        self.tiles.len()
    }
    pub fn rebase(&mut self, eye: DVec3, commands: &mut Commands) {
        if (eye - DVec3::from_array(self.origin.map(|x| f64::from(x) * 16.)))
            .abs()
            .max_element()
            > 128.
        {
            self.origin = origin_for(eye);
            self.rebases += 1;
            for (c, t) in &self.tiles {
                commands
                    .entity(t.entity)
                    .insert(Transform::from_translation(offset(*c, self.origin)));
            }
        }
    }
    pub fn publish(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        material: &super::Surface,
    ) {
        let now = std::time::Instant::now();
        let retired: Vec<_> = self.retired.iter().copied().take(6).collect();
        for c in retired {
            self.retired.remove(&c);
            if let Some(t) = self.tiles.remove(&c) {
                commands.entity(t.entity).despawn();
                self.removed += 1;
            }
        }
        for _ in 0..6 {
            let Some(c) = self.order.pop_front() else {
                break;
            };
            let Some(g) = self.pending.remove(&c) else {
                continue;
            };
            if let Some(t) = self.tiles.remove(&c) {
                commands.entity(t.entity).despawn();
                self.removed += 1;
            }
            if g.quads == 0 {
                continue;
            }
            let mut entity = commands.spawn((
                Mesh3d(meshes.add(g.mesh)),
                Transform::from_translation(offset(c, self.origin)),
                Visibility::Hidden,
            ));
            if !g.forest {
                entity.insert((NoFrustumCulling, MeshMaterial3d(material.historic.clone())));
            } else if let Some(h) = &material.grain {
                entity.insert(MeshMaterial3d(h.clone()));
            } else {
                entity.insert(MeshMaterial3d(material.forest.clone()));
            }
            let entity = entity.id();
            let fl = g.flora;
            let nf = fl.supports.len();
            let flora = [fl.near, fl.far].map(|mesh| {
                mesh.map(|m| {
                    self.uploads += 1;
                    commands
                        .spawn((
                            Mesh3d(meshes.add(m)),
                            MeshMaterial3d(material.flora.clone()),
                            ChildOf(entity),
                            bevy::light::NotShadowCaster,
                            Visibility::Hidden,
                            Transform::default(),
                        ))
                        .id()
                })
            });

            self.tiles.insert(
                c,
                Tile {
                    forest: g.forest,
                    flora,
                    flora_tri: [fl.near_tri, fl.far_tri],
                    plants: [nf, fl.far_count],
                    supports: fl.supports,
                    lod: 2,
                    entity,
                    quads: g.quads,
                    visible: false,
                },
            );
            self.uploads += 1;
            self.bytes += g.bytes;
        }
        self.peak = self.peak.max(self.tiles.len());
        self.publish_us += now.elapsed().as_secs_f64() * 1e6;
    }
    pub fn visibility(
        &mut self,
        eye: DVec3,
        front: Vec3,
        distance: f32,
        aspect: f32,
        veg_distance: f32,
        commands: &mut Commands,
    ) {
        self.flora_tri = 0;
        self.plants = 0;
        self.visible = 0;
        self.quads = 0;
        let right = front.cross(Vec3::Y).normalize_or_zero();
        let up = right.cross(front);
        let radius = 3f32.sqrt() * 8.;
        let vertical = (65f32.to_radians() * 0.5).tan();
        let horiz = vertical * aspect;
        for (&c, t) in &mut self.tiles {
            let delta = (DVec3::from_array(c.map(|v| f64::from(v) * 16. + 8.)) - eye).as_vec3();
            let z = delta.dot(front);
            let visible = !self.retired.contains(&c)
                && delta.length_squared() <= (distance + radius).powi(2)
                && z + radius >= 0.05
                && delta.dot(right).abs() <= z * horiz + radius * (1. + horiz * horiz).sqrt()
                && delta.dot(up).abs() <= z * vertical + radius * (1. + vertical * vertical).sqrt();
            let d = delta.length();
            let lod = if !visible || d > veg_distance + 8. {
                2
            } else if d < veg_distance * 0.62 + 4. {
                0
            } else {
                1
            };
            if lod != t.lod {
                for (i, entity) in t.flora.iter().enumerate() {
                    if let Some(e) = entity {
                        commands.entity(*e).insert(if i == lod {
                            Visibility::Inherited
                        } else {
                            Visibility::Hidden
                        });
                    }
                }
                t.lod = lod;
            }
            if lod < 2 {
                self.flora_tri += t.flora_tri[lod];
                self.plants += t.plants[lod];
            }
            // Keep nearby forest geometry available to the shadow camera even behind the player.
            let submitted = if t.forest {
                !self.retired.contains(&c) && d <= distance + radius
            } else {
                visible
            };
            if submitted != t.visible {
                commands.entity(t.entity).insert(if submitted {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                });
                t.visible = submitted;
            }
            if visible {
                self.visible += 1;
                self.quads += t.quads;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distant_negative_coordinates_preserve_cells_and_subblock_motion() {
        let eye = DVec3::new(-1_500_000_000.125, 32.5, 1_500_000_000.875);
        let c = origin_for(eye);
        let local = (eye - DVec3::from_array(c.map(|x| f64::from(x) * 16.))).as_vec3();
        assert_eq!(local, Vec3::new(15.875, 0.5, 0.875));
        assert_eq!(offset([c[0] + 1, c[1], c[2]], c), Vec3::X * 16.);
    }
}
