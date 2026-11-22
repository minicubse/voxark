use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io,
    path::{Component as PathComponent, Path},
    time::Instant,
};

use bevy::{
    animation::{
        AnimatedBy, AnimationTargetId,
        graph::{AnimationGraph, AnimationGraphHandle, AnimationNodeIndex},
    },
    asset::{AssetLoader, LoadContext, LoadState, RecursiveDependencyLoadState, io::Reader},
    ecs::system::SystemParam,
    gltf::{
        Gltf, GltfLoaderSettings, GltfMaterialName, convert_coordinates::GltfConvertCoordinates,
    },
    mesh::{
        VertexAttributeValues,
        skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
    },
    pbr::MAX_JOINTS,
    prelude::*,
    reflect::TypePath,
    world_serialization::{WorldAsset, WorldAssetRoot, WorldInstance, WorldInstanceSpawner},
};
use serde::{Deserialize, Serialize};

pub const DEFAULT_CREATURE_MANIFEST: &str = "creatures/wyvern.json";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum CreatureClip {
    IdleGround,
    Walk,
    Takeoff,
    Flap,
    Glide,
    Land,
}

impl CreatureClip {
    pub const ALL: [Self; 6] = [
        Self::IdleGround,
        Self::Walk,
        Self::Takeoff,
        Self::Flap,
        Self::Glide,
        Self::Land,
    ];
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CreaturePalette {
    Jade,
    Braise,
    Obsidienne,
}

impl CreaturePalette {
    const ALL: [Self; 3] = [Self::Jade, Self::Braise, Self::Obsidienne];
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
enum MaterialSlot {
    Scales,
    Horns,
    Eyes,
    Membranes,
}

impl MaterialSlot {
    const ALL: [Self; 4] = [Self::Scales, Self::Horns, Self::Eyes, Self::Membranes];
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    model: String,
    scene: Option<String>,
    source_forward: String,
    up_axis: String,
    units: String,
    scale: f32,
    import_correction: String,
    pivot: String,
    root_motion: String,
    clips: BTreeMap<CreatureClip, String>,
    slots: BTreeMap<MaterialSlot, String>,
    palettes: BTreeMap<CreaturePalette, BTreeMap<MaterialSlot, [f32; 3]>>,
}

impl Manifest {
    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "version de manifeste inconnue: {}",
                self.schema_version
            ));
        }
        let path = Path::new(&self.model);
        if !matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("glb" | "gltf")
        ) || self.model.contains([':', '#', '\\'])
            || !path
                .components()
                .all(|part| matches!(part, PathComponent::Normal(_)))
        {
            return Err(format!("chemin GLB/glTF relatif attendu: {}", self.model));
        }
        if self.source_forward != "+Z" {
            return Err(format!(
                "axe avant glTF +Z attendu: {}",
                self.source_forward
            ));
        }
        if self.up_axis != "+Y"
            || self.units != "meters"
            || !self.scale.is_finite()
            || self.scale <= 0.0
        {
            return Err("repère glTF +Y, mètres et échelle positive attendus".into());
        }
        if self.import_correction != "bevy_gltf_scene_rotation" {
            return Err("conversion d’orientation native de la scène glTF attendue".into());
        }
        if self.pivot != "ground" || self.root_motion != "in_place" {
            return Err("pivot au sol et animations sur place attendus".into());
        }
        if self
            .scene
            .as_ref()
            .is_some_and(|scene| scene.trim().is_empty())
        {
            return Err("nom de scène vide".into());
        }
        validate_names("clip", CreatureClip::ALL, &self.clips)?;
        validate_names("matériau", MaterialSlot::ALL, &self.slots)?;
        for palette in CreaturePalette::ALL {
            let colors = self
                .palettes
                .get(&palette)
                .ok_or_else(|| format!("palette {palette:?} manquante"))?;
            for slot in MaterialSlot::ALL {
                let rgb = colors
                    .get(&slot)
                    .ok_or_else(|| format!("couleur {slot:?} manquante dans {palette:?}"))?;
                if !rgb.iter().all(|value| (0.0..=1.0).contains(value)) {
                    return Err(format!(
                        "couleur sRGB hors limites pour {palette:?}/{slot:?}"
                    ));
                }
            }
        }
        Ok(())
    }
}

fn validate_names<K: Copy + Ord + std::fmt::Debug>(
    kind: &str,
    keys: impl IntoIterator<Item = K>,
    names: &BTreeMap<K, String>,
) -> Result<(), String> {
    let mut used = HashSet::new();
    for key in keys {
        let name = names
            .get(&key)
            .ok_or_else(|| format!("{kind} {key:?} manquant"))?;
        if name.trim().is_empty() || !used.insert(name) {
            return Err(format!(
                "nom de {kind} vide ou dupliqué pour {key:?}: {name:?}"
            ));
        }
    }
    Ok(())
}

#[derive(Asset, TypePath)]
struct CreatureDefinition {
    manifest: Manifest,
    #[dependency]
    model: Handle<Gltf>,
}

#[derive(Default, TypePath)]
struct CreatureLoader;

impl AssetLoader for CreatureLoader {
    type Asset = CreatureDefinition;
    type Settings = ();
    type Error = io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        context: &mut LoadContext<'_>,
    ) -> Result<CreatureDefinition, io::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let manifest: Manifest = serde_json::from_slice(&bytes)?;
        manifest.validate().map_err(io::Error::other)?;
        let model = context
            .load_builder()
            .with_settings(|settings: &mut GltfLoaderSettings| {
                settings.convert_coordinates = Some(GltfConvertCoordinates {
                    rotate_scene_entity: true,
                    rotate_meshes: false,
                });
                settings.load_cameras = false;
                settings.load_lights = false;
            })
            .load(manifest.model.clone());
        Ok(CreatureDefinition { manifest, model })
    }

    fn extensions(&self) -> &[&str] {
        &["json"]
    }
}

pub struct CreatureAssetPlugin {
    path: String,
}

impl CreatureAssetPlugin {
    pub fn new(path: String) -> Self {
        Self { path }
    }
}

impl Plugin for CreatureAssetPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<CreatureDefinition>()
            .init_asset_loader::<CreatureLoader>()
            .insert_resource(CreatureAssets {
                path: self.path.clone(),
                definition: None,
                prepared: None,
                counts: CreatureAssetCounts::default(),
                status: CreatureAssetStatus::Loading,
                load_started: None,
            })
            .add_systems(Startup, request_assets)
            .add_systems(
                Update,
                (
                    prepare_assets,
                    mount_creatures,
                    bind_instances,
                    count_instances,
                )
                    .chain(),
            );
    }
}

#[derive(Debug)]
pub enum CreatureAssetStatus {
    Loading,
    Ready,
    Failed(String),
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct CreatureAssetCounts {
    pub scenes: usize,
    pub skins: usize,
    pub clips: usize,
    pub slots: usize,
    pub palette_materials: usize,
    pub mesh_assets: usize,
    pub instances: usize,
    pub players: usize,
    pub skinned_meshes: usize,
    pub max_joints: usize,
    pub asset_load_ms: f64,
}

#[derive(Serialize)]
pub struct CreatureImportInfo {
    pub model: String,
    pub scene: String,
    pub clips: BTreeMap<CreatureClip, String>,
    pub clip_durations_seconds: BTreeMap<CreatureClip, f32>,
    pub scale: f32,
}

#[derive(Resource)]
pub struct CreatureAssets {
    path: String,
    definition: Option<Handle<CreatureDefinition>>,
    prepared: Option<PreparedCreature>,
    counts: CreatureAssetCounts,
    status: CreatureAssetStatus,
    load_started: Option<Instant>,
}

impl CreatureAssets {
    pub fn status(&self) -> &CreatureAssetStatus {
        &self.status
    }

    pub fn counts(&self) -> CreatureAssetCounts {
        self.counts
    }

    pub fn info(&self) -> Option<&CreatureImportInfo> {
        self.prepared.as_ref().map(|model| &model.info)
    }

    pub fn node(&self, clip: CreatureClip) -> Option<AnimationNodeIndex> {
        self.prepared
            .as_ref()
            .map(|model| model.nodes[clip as usize])
    }

    pub fn duration(&self, clip: CreatureClip) -> Option<f32> {
        self.info()?.clip_durations_seconds.get(&clip).copied()
    }

    fn fail(&mut self, reason: &str) {
        let message = format!("Créature, manifeste {}: {reason}", self.path);
        error!("{message}");
        self.status = CreatureAssetStatus::Failed(message);
    }
}

#[derive(Component)]
#[require(Transform, Visibility)]
pub struct CreatureVisual {
    palette: CreaturePalette,
    mount: Option<Entity>,
    ready: bool,
}

impl CreatureVisual {
    pub fn new(palette: CreaturePalette) -> Self {
        Self {
            palette,
            mount: None,
            ready: false,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn palette(&self) -> CreaturePalette {
        self.palette
    }
}

#[derive(Component)]
pub struct CreatureAnimation {
    pub owner: Entity,
}

#[derive(Component)]
pub struct CreatureMesh {
    pub owner: Entity,
    pub membrane: bool,
}

#[derive(Component)]
struct CreatureMount;

#[derive(Component)]
struct BoundInstance {
    skinned_meshes: usize,
}

type PaletteMaterials =
    [[Handle<StandardMaterial>; MaterialSlot::ALL.len()]; CreaturePalette::ALL.len()];

struct PreparedCreature {
    info: CreatureImportInfo,
    scene: Handle<WorldAsset>,
    graph: Handle<AnimationGraph>,
    nodes: Vec<AnimationNodeIndex>,
    slot_names: HashMap<String, MaterialSlot>,
    palettes: PaletteMaterials,
}

#[derive(SystemParam)]
struct ImportAssets<'w> {
    definitions: Res<'w, Assets<CreatureDefinition>>,
    models: Res<'w, Assets<Gltf>>,
    scenes: Res<'w, Assets<WorldAsset>>,
    clips: Res<'w, Assets<AnimationClip>>,
    meshes: Res<'w, Assets<Mesh>>,
    bindposes: Res<'w, Assets<SkinnedMeshInverseBindposes>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    graphs: ResMut<'w, Assets<AnimationGraph>>,
}

fn request_assets(server: Res<AssetServer>, mut assets: ResMut<CreatureAssets>) {
    assets.load_started = Some(Instant::now());
    assets.definition = Some(server.load(assets.path.clone()));
}

fn prepare_assets(
    server: Res<AssetServer>,
    mut assets: ResMut<CreatureAssets>,
    mut source: ImportAssets,
) {
    if !matches!(assets.status, CreatureAssetStatus::Loading) {
        return;
    }
    let Some(definition) = assets.definition.as_ref() else {
        return;
    };
    let id = definition.id();
    if let Some(LoadState::Failed(error)) = server.get_load_state(id) {
        assets.fail(&error.to_string());
        return;
    }
    if let Some(RecursiveDependencyLoadState::Failed(error)) =
        server.get_recursive_dependency_load_state(id)
    {
        assets.fail(&error.to_string());
        return;
    }
    if !server.is_loaded_with_dependencies(id) {
        return;
    }
    let asset_load_ms = assets
        .load_started
        .map_or(0., |start| start.elapsed().as_secs_f64() * 1_000.);
    match source.prepare(definition) {
        Ok((prepared, mut counts)) => {
            info!(
                "Créature chargée: {} squelette(s), {} clips, {} matériaux de palette, {} maillages partagés",
                counts.skins, counts.clips, counts.palette_materials, counts.mesh_assets
            );
            assets.prepared = Some(prepared);
            counts.asset_load_ms = asset_load_ms;
            assets.counts = counts;
            assets.status = CreatureAssetStatus::Ready;
        }
        Err(error) => assets.fail(&error),
    }
}

impl ImportAssets<'_> {
    fn prepare(
        &mut self,
        definition: &Handle<CreatureDefinition>,
    ) -> Result<(PreparedCreature, CreatureAssetCounts), String> {
        let definition = self
            .definitions
            .get(definition)
            .ok_or("manifeste indisponible")?;
        let manifest = &definition.manifest;
        let model = self
            .models
            .get(&definition.model)
            .ok_or("GLB/glTF indisponible")?;
        let scene = match &manifest.scene {
            Some(name) => model.named_scenes.get(name.as_str()),
            None => model.default_scene.as_ref(),
        }
        .ok_or_else(|| {
            format!(
                "scène introuvable dans {}: {:?}",
                manifest.model, manifest.scene
            )
        })?;
        let template = &self.scenes.get(scene).ok_or("scène indisponible")?.world;
        let mut players = template
            .iter_entities()
            .filter(|entity| entity.contains::<AnimationPlayer>());
        let player = players
            .next()
            .ok_or("lecteur d’animations absent de la scène")?
            .id();
        if players.next().is_some() {
            return Err("la scène doit contenir un seul lecteur d’animations".into());
        }
        let targets: HashSet<_> = template
            .iter_entities()
            .filter(|entity| entity.get::<AnimatedBy>().is_some_and(|by| by.0 == player))
            .filter_map(|entity| entity.get::<AnimationTargetId>().copied())
            .collect();
        let mut skin_targets = HashSet::new();
        let mut skin_ids = HashSet::new();
        let mut mesh_ids = HashSet::new();
        let mut used_slots = HashMap::new();
        let slot_names: HashMap<_, _> = manifest
            .slots
            .iter()
            .map(|(&slot, name)| (name.clone(), slot))
            .collect();
        let mut scene_name = None;
        let mut max_joints = 0;
        for entity in template.iter_entities() {
            if !entity.contains::<ChildOf>()
                && let Some(name) = entity.get::<Name>()
            {
                scene_name = Some(name.as_str().to_owned());
            }
            if let Some(mesh) = entity.get::<Mesh3d>() {
                mesh_ids.insert(mesh.id());
            }
            if let Some(name) = entity.get::<GltfMaterialName>()
                && slot_names.contains_key(&name.0)
                && let Some(material) = entity.get::<MeshMaterial3d<StandardMaterial>>()
                && used_slots
                    .insert(name.0.as_str(), &material.0)
                    .is_some_and(|previous| previous != &material.0)
            {
                return Err(format!("plusieurs matériaux PBR pour le nom {}", name.0));
            }
            let Some(skin) = entity.get::<SkinnedMesh>() else {
                continue;
            };
            let bindposes = self
                .bindposes
                .get(&skin.inverse_bindposes)
                .ok_or("poses inverses absentes")?;
            if skin.joints.is_empty()
                || skin.joints.len() > MAX_JOINTS
                || bindposes.len() != skin.joints.len()
                || bindposes.iter().any(|matrix| !matrix.is_finite())
            {
                return Err(format!(
                    "squelette invalide: {} articulations",
                    skin.joints.len()
                ));
            }
            for &joint in &skin.joints {
                let entity = template
                    .get_entity(joint)
                    .map_err(|_| "articulation hors scène")?;
                let target = entity
                    .get::<AnimationTargetId>()
                    .ok_or("articulation sans cible d’animation")?;
                if entity.get::<AnimatedBy>().is_none_or(|by| by.0 != player) {
                    return Err("articulation hors du lecteur d’animations de la scène".into());
                }
                skin_targets.insert(*target);
            }
            let mesh = entity
                .get::<Mesh3d>()
                .and_then(|mesh| self.meshes.get(&mesh.0))
                .ok_or("maillage skinné indisponible")?;
            validate_joint_weights(mesh, skin.joints.len())?;
            max_joints = max_joints.max(skin.joints.len());
            skin_ids.insert(skin.inverse_bindposes.id());
        }
        if skin_ids.is_empty() {
            return Err("aucun maillage skinné dans la scène".into());
        }
        let mut clips = Vec::new();
        let mut clip_durations_seconds = BTreeMap::new();
        for clip in CreatureClip::ALL {
            let name = &manifest.clips[&clip];
            let handle = model
                .named_animations
                .get(name.as_str())
                .ok_or_else(|| format!("clip {clip:?} introuvable: {name}"))?;
            let asset = self
                .clips
                .get(handle)
                .ok_or_else(|| format!("clip indisponible: {name}"))?;
            let duration = asset.duration();
            if !duration.is_finite()
                || duration <= 0.0
                || !asset.curves().keys().all(|target| targets.contains(target))
                || !asset
                    .curves()
                    .keys()
                    .any(|target| skin_targets.contains(target))
            {
                return Err(format!(
                    "clip {name} invalide pour le squelette de la scène"
                ));
            }
            clips.push(handle.clone());
            clip_durations_seconds.insert(clip, duration);
        }
        let mut base_materials = Vec::new();
        for slot in MaterialSlot::ALL {
            let name = &manifest.slots[&slot];
            let base = used_slots
                .get(name.as_str())
                .and_then(|handle| self.materials.get(*handle))
                .ok_or_else(|| format!("matériau PBR {slot:?} absent de la scène: {name}"))?;
            base_materials.push(base.clone());
        }
        let palettes = build_palettes(manifest, &base_materials, &mut self.materials);
        let (graph, nodes) = AnimationGraph::from_clips(clips);
        Ok((
            PreparedCreature {
                info: CreatureImportInfo {
                    model: manifest.model.clone(),
                    scene: scene_name.unwrap_or_else(|| "Scene0".into()),
                    clips: manifest.clips.clone(),
                    clip_durations_seconds,
                    scale: manifest.scale,
                },
                scene: scene.clone(),
                graph: self.graphs.add(graph),
                nodes,
                slot_names,
                palettes,
            },
            CreatureAssetCounts {
                scenes: 1,
                skins: skin_ids.len(),
                clips: CreatureClip::ALL.len(),
                slots: MaterialSlot::ALL.len(),
                palette_materials: CreaturePalette::ALL.len() * MaterialSlot::ALL.len(),
                mesh_assets: mesh_ids.len(),
                max_joints,
                ..default()
            },
        ))
    }
}

fn build_palettes(
    manifest: &Manifest,
    base_materials: &[StandardMaterial],
    materials: &mut Assets<StandardMaterial>,
) -> PaletteMaterials {
    CreaturePalette::ALL.map(|palette| {
        MaterialSlot::ALL.map(|slot| {
            let rgb = manifest.palettes[&palette][&slot];
            let mut material = base_materials[slot as usize].clone();
            material.base_color = Color::srgba(rgb[0], rgb[1], rgb[2], material.base_color.alpha());
            materials.add(material)
        })
    })
}

fn validate_joint_weights(mesh: &Mesh, joint_count: usize) -> Result<(), String> {
    let (
        Some(VertexAttributeValues::Uint16x4(joints)),
        Some(VertexAttributeValues::Float32x4(weights)),
    ) = (
        mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX),
        mesh.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT),
    )
    else {
        return Err("indices ou poids d’articulation manquants".into());
    };
    if joints.is_empty()
        || joints.len() != weights.len()
        || joints
            .iter()
            .flatten()
            .any(|&joint| usize::from(joint) >= joint_count)
        || weights.iter().any(|weights| {
            weights.iter().any(|weight| !(0.0..=1.0).contains(weight))
                || (weights.iter().sum::<f32>() - 1.0).abs() > 0.015
        })
    {
        return Err("indices ou poids d’articulation invalides".into());
    }
    Ok(())
}

fn mount_creatures(
    mut commands: Commands,
    assets: Res<CreatureAssets>,
    mut creatures: Query<(Entity, &mut CreatureVisual)>,
) {
    if !matches!(assets.status, CreatureAssetStatus::Ready) {
        return;
    }
    let Some(model) = assets.prepared.as_ref() else {
        return;
    };
    for (owner, mut visual) in &mut creatures {
        if visual.mount.is_none() {
            visual.mount = Some(
                commands
                    .spawn((
                        Name::new("Wyverne glTF"),
                        CreatureMount,
                        WorldAssetRoot(model.scene.clone()),
                        Transform::from_scale(Vec3::splat(model.info.scale)),
                        ChildOf(owner),
                    ))
                    .id(),
            );
        }
    }
}

#[derive(SystemParam)]
struct InstanceParts<'w, 's> {
    players: Query<'w, 's, (), With<AnimationPlayer>>,
    joints: Query<'w, 's, &'static AnimatedBy, With<AnimationTargetId>>,
    meshes: Query<
        'w,
        's,
        (
            Option<&'static GltfMaterialName>,
            Option<&'static SkinnedMesh>,
        ),
        With<Mesh3d>,
    >,
}

type UnboundMounts<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static ChildOf, &'static WorldInstance),
    (With<CreatureMount>, Without<BoundInstance>),
>;

fn bind_instances(
    mut commands: Commands,
    spawner: Res<WorldInstanceSpawner>,
    mut assets: ResMut<CreatureAssets>,
    mounts: UnboundMounts,
    mut visuals: Query<&mut CreatureVisual>,
    parts: InstanceParts,
) {
    if !matches!(assets.status, CreatureAssetStatus::Ready) {
        return;
    }
    let Some(model) = assets.prepared.as_ref() else {
        return;
    };
    for (mount, parent, instance) in &mounts {
        if !spawner.instance_is_ready(**instance) {
            continue;
        }
        let owner = parent.parent();
        let Ok(mut visual) = visuals.get_mut(owner) else {
            commands.entity(mount).despawn();
            continue;
        };
        let entities: HashSet<_> = spawner.iter_instance_entities(**instance).collect();
        let mut players = entities
            .iter()
            .copied()
            .filter(|&entity| parts.players.contains(entity));
        let Some(player) = players.next() else {
            assets.fail("lecteur d’animations absent de l’instance");
            return;
        };
        let mut skinned_meshes = 0;
        for &entity in &entities {
            let Ok((_, Some(skin))) = parts.meshes.get(entity) else {
                continue;
            };
            if skin.joints.iter().any(|&joint| {
                !entities.contains(&joint)
                    || !matches!(parts.joints.get(joint), Ok(by) if by.0 == player)
            }) {
                assets.fail("articulation de l’instance liée à un autre lecteur d’animations");
                return;
            }
            skinned_meshes += 1;
        }
        if players.next().is_some() || skinned_meshes == 0 {
            assets.fail("l’instance doit contenir un lecteur d’animations et un maillage skinné");
            return;
        }
        for &entity in &entities {
            let Ok((name, _)) = parts.meshes.get(entity) else {
                continue;
            };
            let mut mesh = commands.entity(entity);
            let slot = name.and_then(|name| model.slot_names.get(&name.0));
            mesh.insert(CreatureMesh {
                owner,
                membrane: slot == Some(&MaterialSlot::Membranes),
            });
            if let Some(slot) = slot {
                let handle = model.palettes[visual.palette as usize][*slot as usize].clone();
                mesh.insert(MeshMaterial3d(handle));
            }
        }
        commands.entity(player).insert((
            AnimationGraphHandle(model.graph.clone()),
            AnimationTransitions::new(),
            CreatureAnimation { owner },
        ));
        commands
            .entity(mount)
            .insert(BoundInstance { skinned_meshes });
        visual.ready = true;
    }
}

fn count_instances(mut assets: ResMut<CreatureAssets>, instances: Query<&BoundInstance>) {
    let counts = &mut assets.bypass_change_detection().counts;
    counts.instances = 0;
    counts.skinned_meshes = 0;
    for instance in &instances {
        counts.instances += 1;
        counts.skinned_meshes += instance.skinned_meshes;
    }
    counts.players = counts.instances;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        serde_json::from_str(include_str!("../../../assets/creatures/wyvern.json")).unwrap()
    }

    #[test]
    fn manifest_requires_all_clips_and_named_slots() {
        let mut manifest = manifest();
        assert!(manifest.validate().is_ok());
        manifest.clips.remove(&CreatureClip::Land);
        assert!(manifest.validate().unwrap_err().contains("Land"));
        manifest.clips.insert(CreatureClip::Land, "Land".into());
        manifest.slots.insert(MaterialSlot::Eyes, "Scales".into());
        assert!(manifest.validate().unwrap_err().contains("dupliqué"));
    }

    #[test]
    fn manifest_rejects_invalid_path_axes_and_pivot() {
        let mut manifest = manifest();
        manifest.model = "../escape.glb".into();
        assert!(manifest.validate().unwrap_err().contains("chemin"));
        manifest.model = "creatures/wyvern.glb".into();
        manifest.source_forward = "-Z".into();
        assert!(manifest.validate().unwrap_err().contains("+Z"));
        manifest.source_forward = "+Z".into();
        manifest.root_motion = "animated".into();
        assert!(manifest.validate().unwrap_err().contains("sur place"));
    }

    #[test]
    fn palette_materials_preserve_pbr_fields_and_have_independent_handles() {
        let mut materials = Assets::<StandardMaterial>::default();
        let base = StandardMaterial {
            base_color: Color::srgba(1., 1., 1., 0.67),
            perceptual_roughness: 0.82,
            metallic: 0.21,
            ..default()
        };
        let palettes = build_palettes(
            &manifest(),
            &MaterialSlot::ALL.map(|_| base.clone()),
            &mut materials,
        );
        let ids: HashSet<_> = palettes.iter().flatten().map(Handle::id).collect();
        assert_eq!(ids.len(), 12);
        for material in palettes.iter().flatten() {
            let material = materials.get(material).unwrap();
            assert_eq!(material.base_color.alpha(), base.base_color.alpha());
            assert_eq!(material.perceptual_roughness, base.perceptual_roughness);
            assert_eq!(material.metallic, base.metallic);
        }
        let color = |materials: &Assets<StandardMaterial>, palette: CreaturePalette| {
            materials
                .get(&palettes[palette as usize][MaterialSlot::Scales as usize])
                .unwrap()
                .base_color
        };
        let braise = color(&materials, CreaturePalette::Braise);
        let obsidienne = color(&materials, CreaturePalette::Obsidienne);
        materials
            .get_mut(&palettes[CreaturePalette::Jade as usize][MaterialSlot::Scales as usize])
            .unwrap()
            .base_color = Color::BLACK;
        assert_eq!(color(&materials, CreaturePalette::Braise), braise);
        assert_eq!(color(&materials, CreaturePalette::Obsidienne), obsidienne);
    }

    #[test]
    fn rejects_out_of_range_joint_indices_and_unweighted_vertices() {
        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_JOINT_INDEX,
            VertexAttributeValues::Uint16x4(vec![[0, 1, 0, 0]]),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, vec![[0.75, 0.25, 0., 0.]]);
        assert!(validate_joint_weights(&mesh, 2).is_ok());
        assert!(validate_joint_weights(&mesh, 1).is_err());
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, vec![[0., 0., 0., 0.]]);
        assert!(validate_joint_weights(&mesh, 2).is_err());
    }
}
