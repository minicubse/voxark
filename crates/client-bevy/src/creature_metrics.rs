//! Development instrumentation. Keep expensive set construction outside timed frames.
use crate::creature_asset::{CreatureMesh, CreaturePalette, CreatureVisual};
use bevy::{
    mesh::{
        Indices,
        skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
    },
    prelude::*,
    render::{Render, RenderApp, RenderSystems, mesh::RenderMesh, render_asset::RenderAssets},
};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Instant,
};

#[derive(Default)]
struct Receipt {
    ids: Vec<AssetId<Mesh>>,
    published: Option<Instant>,
    discovered: Option<Instant>,
}
#[derive(Resource, Clone, Default)]
struct GpuReceipt(Arc<Mutex<Receipt>>);
#[derive(Resource, Default)]
pub struct Inventory(pub Value);

#[derive(Resource, Default)]
pub struct Lifecycle(pub Value);

pub struct CreatureMetricsPlugin;
impl Plugin for CreatureMetricsPlugin {
    fn build(&self, app: &mut App) {
        let receipt = GpuReceipt::default();
        app.insert_resource(receipt.clone())
            .init_resource::<Inventory>()
            .init_resource::<Lifecycle>()
            .add_systems(
                Update,
                lifecycle.run_if(|o: Res<super::Options>| o.creature_lifecycle),
            )
            .add_systems(
                Update,
                (discover_meshes, inventory.run_if(last_frame)).before(super::measure),
            );
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.insert_resource(receipt).add_systems(
                Render,
                check_publication.after(RenderSystems::PrepareAssets),
            );
        }
    }
}
fn last_frame(bench: Res<super::Bench>, opts: Res<super::Options>) -> bool {
    opts.frames > 0 && bench.warm >= 120 && bench.samples.len() + 1 >= opts.frames
}
fn discover_meshes(
    q: Query<&Mesh3d, With<SkinnedMesh>>,
    receipt: Res<GpuReceipt>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let ids: HashSet<_> = q.iter().map(|m| m.id()).collect();
    if !ids.is_empty() {
        let mut receipt = receipt.0.lock().unwrap();
        receipt.ids.extend(ids);
        receipt.discovered = Some(Instant::now());
        *done = true;
    }
}
fn check_publication(
    meshes: Res<RenderAssets<RenderMesh>>,
    receipt: Res<GpuReceipt>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let mut r = receipt.0.lock().unwrap();
    if !r.ids.is_empty() && r.ids.iter().all(|&id| meshes.get(id).is_some()) {
        r.published = Some(Instant::now());
        *done = true;
    }
}
#[allow(
    clippy::too_many_arguments,
    reason = "Independent Bevy asset inventories"
)]
fn inventory(
    camera: Single<&bevy::camera::visibility::VisibleEntities, With<Camera3d>>,
    parts: Query<&CreatureMesh>,
    skins: Query<(
        &Mesh3d,
        &SkinnedMesh,
        &MeshMaterial3d<StandardMaterial>,
        &ViewVisibility,
    )>,
    graphs: Query<&bevy::animation::graph::AnimationGraphHandle, With<AnimationPlayer>>,
    meshes: Res<Assets<Mesh>>,
    binds: Res<Assets<SkinnedMeshInverseBindposes>>,
    clips: Res<Assets<AnimationClip>>,
    receipt: Res<GpuReceipt>,
    mut result: ResMut<Inventory>,
) {
    let in_main_view: HashSet<_> = camera
        .get(std::any::TypeId::of::<Mesh3d>())
        .iter()
        .filter_map(|e| parts.get(*e).ok().map(|p| p.owner))
        .collect();
    let mut mesh_ids = HashSet::new();
    let mut joint_ids = HashSet::new();
    let mut bind_ids = HashSet::new();
    let mut materials = HashSet::new();
    let mut triangles = 0;
    let mut visible_triangles = 0;
    let mut visible_primitives = 0;
    let mut joint_refs = 0;
    let mut mesh_bytes = 0;
    let mut unique_triangles = 0;
    for (mesh, skin, mat, visible) in &skins {
        let m = meshes
            .get(mesh.id())
            .expect("mesh principal de la créature");
        let indices = m.indices();
        let tri = indices.map_or(m.count_vertices(), Indices::len) / 3;
        if mesh_ids.insert(mesh.id()) {
            mesh_bytes += m.get_vertex_buffer_size()
                + indices.map_or(0, |i| match i {
                    Indices::U16(v) => v.len() * 2,
                    Indices::U32(v) => v.len() * 4,
                });
            unique_triangles += tri;
        }
        if visible.get() {
            visible_primitives += 1;
            visible_triangles += tri;
        }
        triangles += tri;
        joint_ids.extend(skin.joints.iter().copied());
        joint_refs += skin.joints.capacity() * size_of::<Entity>();
        bind_ids.insert(skin.inverse_bindposes.id());
        materials.insert(mat.id());
    }
    let bind_bytes: usize = bind_ids
        .iter()
        .filter_map(|id| binds.get(*id))
        .map(|b| b.len() * 64)
        .sum();
    let graphs: HashSet<_> = graphs.iter().map(|g| g.0.id()).collect();
    let r = receipt.0.lock().unwrap();
    result.0 = json!({
        "primary_view_instances":in_main_view.len(),"unique_meshes":mesh_ids.len(),"unique_mesh_bytes":mesh_bytes,"unique_triangles":unique_triangles,
        "unique_inverse_bind_assets":bind_ids.len(),"inverse_bind_bytes":bind_bytes,"unique_materials_used":materials.len(),
        "unique_graphs":graphs.len(),"loaded_clips":clips.len(),"joints_all_instances":joint_ids.len(),
        "joint_transform_bytes":joint_ids.len()*(size_of::<Transform>()+size_of::<GlobalTransform>()),
        "joint_reference_bytes":joint_refs,"skinned_primitives":skins.iter().count(),"view_visible_primitives":visible_primitives,
        "triangles_all_instances":triangles,"view_visible_triangles":visible_triangles,
        "gpu_resident_meshes":r.published.map(|_|r.ids.len()),"scene_to_render_asset_ms":r.discovered.zip(r.published).map(|(a,b)|b.saturating_duration_since(a).as_secs_f64()*1000.)
    });
}

#[derive(Default)]
struct Removed {
    root: Option<Entity>,
    joints: Vec<Entity>,
    originals: Vec<(CreaturePalette, AssetId<StandardMaterial>, Color)>,
}
#[allow(
    clippy::too_many_arguments,
    reason = "Development-only verification of live scene and cached asset handles"
)]
fn lifecycle(
    mut cmds: Commands,
    bench: Res<super::Bench>,
    roots: Query<(Entity, &CreatureVisual)>,
    parts: Query<(
        &CreatureMesh,
        &MeshMaterial3d<StandardMaterial>,
        &bevy::gltf::GltfMaterialName,
        &SkinnedMesh,
    )>,
    exists: Query<Entity>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut removed: Local<Removed>,
    mut outcome: ResMut<Lifecycle>,
) {
    if bench.warm < 120 {
        return;
    }
    if removed.root.is_none() && bench.samples.len() >= 8 {
        let mut palettes = std::collections::BTreeMap::new();
        for (part, mat, name, _) in &parts {
            if name.0 == "Scales" {
                let palette = roots.get(part.owner).unwrap().1.palette();
                assert!(roots.get(part.owner).unwrap().1.is_ready());
                let previous = palettes.insert(palette, mat.id());
                assert!(previous.is_none_or(|p| p == mat.id()), "palette dupliquée");
            }
        }
        assert_eq!(palettes.len(), 3);
        let jade = palettes[&CreaturePalette::Jade];
        removed.originals = palettes
            .iter()
            .map(|(&p, &h)| (p, h, mats.get(h).unwrap().base_color))
            .collect();
        mats.get_mut(jade).unwrap().base_color = Color::srgb(1., 0., 1.);
        for &(p, h, c) in &removed.originals {
            assert_eq!(
                mats.get(h).unwrap().base_color == c,
                p != CreaturePalette::Jade
            );
        }
        mats.get_mut(jade).unwrap().base_color = removed
            .originals
            .iter()
            .find(|r| r.0 == CreaturePalette::Jade)
            .unwrap()
            .2;
        let root = roots
            .iter()
            .find(|(_, v)| v.palette() == CreaturePalette::Jade)
            .unwrap()
            .0;
        removed.joints = parts
            .iter()
            .filter(|(p, _, _, _)| p.owner == root)
            .flat_map(|(_, _, _, s)| s.joints.iter().copied())
            .collect();
        assert!(!removed.joints.is_empty());
        removed.root = Some(root);
        cmds.entity(root).despawn();
    } else if let Some(root) = removed.root
        && bench.samples.len() >= 20
        && outcome.0.is_null()
    {
        assert!(
            exists.get(root).is_err() && removed.joints.iter().all(|j| exists.get(*j).is_err())
        );
        assert!(parts.iter().all(|(p, _, _, _)| p.owner != root));
        assert!(
            removed
                .originals
                .iter()
                .all(|(_, id, c)| mats.get(*id).is_some_and(|m| m.base_color == *c))
        );
        outcome.0 = json!({"palette_mutation_isolated":true,"original_colors_restored":true,
            "root_and_skin_joints_despawned":true,"remaining_instances":roots.iter().count(),
            "shared_palette_materials_retained":true});
    }
}
