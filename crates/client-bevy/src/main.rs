mod creature_asset;
mod creature_motion;
mod daylight;
mod flora;
mod forest;
mod network;
mod terrain;

use bevy::{
    core_pipeline::tonemapping::Tonemapping,
    input::mouse::AccumulatedMouseMotion,
    math::DVec3,
    prelude::*,
    render::{
        renderer::RenderAdapterInfo,
        view::screenshot::{Screenshot, save_to_disk},
    },
    window::{CursorGrabMode, CursorOptions, PresentMode, WindowResolution},
    winit::WinitSettings,
};
use primal_storage::RegionStore;
use primal_streaming::{MeshKind, StreamConfig, Streamer, View};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};
use terrain::{Geometry, Terrain};

#[derive(Resource)]
struct Options {
    size: (u32, u32),
    radius: i32,
    frames: usize,
    report: Option<PathBuf>,
    capture: Option<PathBuf>,
    route: String,
    pbr: bool,
    preset: Option<primal_voxel::Preset>,
    style: forest::Style,
    view: Option<[f64; 5]>,
    world: Option<PathBuf>,
    connect: Option<String>,
    identity: PathBuf,
    input_hz: u32,
    playtest: bool,
    creatures: usize,
    creature_manifest: String,
    asset_root: Option<PathBuf>,
    creature_state: String,
    creature_time: Option<f64>,
    creature_speed: f32,
    creature_shadows: bool,
    creature_pause: bool,
    creature_probe: bool,
    creature_profile: bool,
    creature_placement: String,
}
impl Options {
    fn parse() -> Self {
        let mut o = Self {
            size: (1280, 720),
            radius: 4,
            frames: 0,
            report: None,
            capture: None,
            route: "fixed".into(),
            pbr: false,
            preset: None,
            style: forest::Style::default(),
            view: None,
            world: None,
            connect: None,
            identity: "bevy.identity".into(),
            input_hz: 60,
            playtest: false,
            creatures: 0,
            creature_manifest: creature_asset::DEFAULT_CREATURE_MANIFEST.into(),
            asset_root: None,
            creature_state: "demo".into(),
            creature_time: None,
            creature_speed: 1.,
            creature_shadows: true,
            creature_pause: false,
            creature_probe: false,
            creature_profile: false,
            creature_placement: "near".into(),
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            if arg == "--test-input" {
                o.playtest = true;
                continue;
            }
            if arg == "--compare" {
                continue;
            }
            let val = args
                .next()
                .unwrap_or_else(|| panic!("Valeur manquante pour {arg}"));
            match arg.as_str() {
                "--creatures" => o.creatures = val.parse().expect("nombre de créatures"),
                "--asset-root" => o.asset_root = Some(val.into()),
                "--creature-manifest" => o.creature_manifest = val,
                "--creature-state" => o.creature_state = val.to_ascii_lowercase(),
                "--creature-time" => {
                    o.creature_time = Some(val.parse().expect("temps de démonstration"))
                }
                "--creature-speed" => o.creature_speed = val.parse().expect("vitesse d’animation"),
                "--creature-shadows" => o.creature_shadows = switch(&val),
                "--creature-pause" => o.creature_pause = switch(&val),
                "--creature-profile" => o.creature_profile = switch(&val),
                "--creature-probe" => o.creature_probe = switch(&val),
                "--creature-placement" => o.creature_placement = val,
                "--connect" => o.connect = Some(val),
                "--identity" => o.identity = val.into(),
                "--input-hz" => o.input_hz = val.parse().unwrap(),
                "--resolution" => {
                    let (w, h) = val.split_once('x').expect("WIDTHxHEIGHT");
                    o.size = (w.parse().unwrap(), h.parse().unwrap());
                }
                "--render-radius" => o.radius = val.parse().unwrap(),
                "--frames" | "--net-frames" => o.frames = val.parse().unwrap(),
                "--report" | "--net-report" => o.report = Some(val.into()),
                "--capture" => o.capture = Some(val.into()),
                "--preset" => {
                    o.preset = Some(match val.as_str() {
                        "historical" => primal_voxel::Preset::Historical,
                        "forest-modern" => primal_voxel::Preset::ForestModern,
                        _ => panic!("Preset inconnu"),
                    })
                }
                "--palette" => o.style.palette = switch(&val),
                "--variation" => o.style.variation = switch(&val),
                "--culling" => o.style.cull = switch(&val),
                "--view" => {
                    o.view = Some(
                        val.split(',')
                            .map(|v| v.parse::<f64>().expect("coordonnée de caméra"))
                            .collect::<Vec<_>>()
                            .try_into()
                            .expect("x,y,z,yaw,pitch"),
                    )
                }
                "--density" => o.style.density = val.parse().unwrap(),
                "--vegetation-distance" => o.style.veg_distance = val.parse().unwrap(),
                "--ao" => o.style.ao = switch(&val),
                "--lighting" => o.style.lighting = switch(&val),
                "--fog" => o.style.fog = switch(&val),
                "--shadows" => o.style.shadows = val.parse().unwrap(),
                "--shadow-size" => o.style.shadow_size = val.parse().unwrap(),
                "--shadow-distance" => o.style.shadow_distance = val.parse().unwrap(),
                "--tone" => {
                    o.style.tone = match val.as_str() {
                        "agx" => Tonemapping::AgX,
                        "tony" => Tonemapping::TonyMcMapface,
                        "none" => Tonemapping::None,
                        _ => panic!("tone: none|agx|tony"),
                    }
                }
                "--world" => o.world = Some(val.into()),
                "--route" | "--compare-route" => {
                    assert!(matches!(
                        val.as_str(),
                        "fixed" | "drift" | "forest-drift" | "forest-traverse" | "traverse" | "far"
                    ));
                    o.route = val;
                }
                "--material" => {
                    assert!(matches!(val.as_str(), "simple" | "pbr"));
                    o.pbr = val == "pbr";
                }
                _ => panic!("Option inconnue {arg}"),
            }
        }
        assert!(
            (0.0..=1.0).contains(&o.style.density)
                && (0.0..=368.).contains(&o.style.veg_distance)
                && o.style.shadows <= 2
                && matches!(o.style.shadow_size, 512 | 1024 | 2048)
                && (4.0..=150.).contains(&o.style.shadow_distance)
        );
        assert!((1..=23).contains(&o.radius) && (10..=120).contains(&o.input_hz));
        assert!(
            o.connect.is_none() || o.world.is_none(),
            "--world est réservé au solo"
        );
        assert!(o.creatures <= 64 && (0.05..=8.0).contains(&o.creature_speed));
        assert!(o.creature_time.is_none_or(|v| v.is_finite() && v >= 0.));
        assert!(matches!(
            o.creature_state.as_str(),
            "demo" | "idleground" | "walk" | "takeoff" | "flap" | "glide" | "land"
        ));
        assert!(matches!(
            o.creature_placement.as_str(),
            "near" | "far" | "offscreen"
        ));
        if o.creatures > 0 {
            assert!(
                o.connect.is_none(),
                "L’inspection des créatures est une démonstration locale"
            );
            assert!(
                o.preset
                    .is_none_or(|p| p == primal_voxel::Preset::ForestModern),
                "La démonstration utilise forest-modern"
            );
            o.preset = Some(primal_voxel::Preset::ForestModern);
        }
        o
    }
}
fn switch(value: &str) -> bool {
    match value {
        "on" => true,
        "off" => false,
        _ => panic!("Option attend on ou off"),
    }
}
#[derive(Resource)]
struct LocalWorld {
    stream: Mutex<Streamer<Geometry>>,
    _dir: Option<tempfile::TempDir>,
}
#[derive(Resource)]
struct CameraRig {
    eye: DVec3,
    yaw: f32,
    pitch: f32,
    velocity: Vec3,
}
impl CameraRig {
    fn front(&self) -> Vec3 {
        Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        )
    }
}
#[derive(Resource)]
struct Surface {
    flora: Handle<StandardMaterial>,
    historic: Handle<StandardMaterial>,
    forest: Handle<StandardMaterial>,
    grain: Option<Handle<forest::ForestMaterial>>,
}
#[derive(Resource)]
struct Bench {
    last: Instant,
    start: Instant,
    stable: Instant,
    signature: (usize, usize, usize),
    warm: usize,
    samples: Vec<f64>,
    cpu: cpu_time::ProcessTime,
    empty: usize,
    geometry: u64,
    inputs: u64,
    baseline: (usize, usize, f64),
    shot: bool,
    creature_ready_ms: Option<f64>,
}
fn main() -> AppExit {
    let mut o = Options::parse();
    let mut app = App::new();
    let mut camera_seed = 20_261_120;
    if let Some(address) = &o.connect {
        let token = primal_net::identity(&o.identity).expect("Identité réseau");
        app.insert_resource(network::Online(Mutex::new(network::Session::new(
            address.clone(),
            token,
            o.input_hz,
            o.playtest,
            o.style,
            o.style.lighting,
        ))));
    } else {
        let dir = o.world.is_none().then(|| tempfile::tempdir().unwrap());
        let path = o
            .world
            .clone()
            .unwrap_or_else(|| dir.as_ref().unwrap().path().join("world.pvx"));
        let store =
            RegionStore::open_with_preset(&path, 20_261_120, o.preset).expect("ouverture du monde");
        let preset = store.preset();
        o.preset = Some(preset);
        let seed = store.seed();
        camera_seed = seed;
        let cfg = StreamConfig {
            load_radius: o.radius + 1,
            render_radius: o.radius,
            uploads_per_tick: 6,
            ..default()
        };
        let world = primal_voxel::World::with_preset(seed, preset);
        let style = o.style;
        let lit = style.lighting;
        let local_stream = if preset == primal_voxel::Preset::ForestModern {
            Streamer::new_with_snapshot(world, store, cfg, move |s| {
                forest::prepare(s, style, lit, seed)
            })
        } else {
            Streamer::new(world, store, cfg, MeshKind::GreedyLocal, terrain::prepare)
        }
        .unwrap();
        app.insert_resource(LocalWorld {
            stream: Mutex::new(local_stream),
            _dir: dir,
        });
    }
    let eye = if o.route == "far" {
        DVec3::new(-1_500_000_000.375, 32., 1_499_999_999.625)
    } else {
        DVec3::new(0.5, 18., 0.5)
    };
    let [x, y, z, yaw, pitch] = o.view.unwrap_or_else(|| {
        if o.preset == Some(primal_voxel::Preset::ForestModern) {
            [
                0.5,
                primal_voxel::surface_for(primal_voxel::Preset::ForestModern, camera_seed, 0, 0)
                    as f64
                    + 3.,
                0.5,
                0.78,
                -0.10,
            ]
        } else {
            [eye.x, eye.y, eye.z, 0.78, -0.32]
        }
    });
    let now = Instant::now();
    // A portable build resolves its assets next to the executable, including when launched elsewhere.
    let portable_assets = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("assets")));
    let assets = o
        .asset_root
        .clone()
        .or_else(|| portable_assets.filter(|p| p.is_dir()))
        .unwrap_or_else(|| {
            std::env::current_dir()
                .expect("dossier courant")
                .join("assets")
        });
    let creatures = o.creatures;
    let profiling = o.creature_profile;
    let measuring = o.frames > 0;
    let manifest = o.creature_manifest.clone();
    app.add_plugins(
        DefaultPlugins
            .set(AssetPlugin {
                file_path: std::path::absolute(assets)
                    .expect("dossier des assets")
                    .to_string_lossy()
                    .into_owned(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Primal Voxel — Bevy".into(),
                    resolution: WindowResolution::new(o.size.0, o.size.1)
                        .with_scale_factor_override(1.),
                    present_mode: PresentMode::Immediate,
                    ..default()
                }),
                ..default()
            }),
    )
    .add_plugins(forest::ForestPlugin)
    .insert_resource(WinitSettings::continuous())
    .insert_resource(o)
    .insert_resource(CameraRig {
        eye: DVec3::new(x, y, z),
        yaw: yaw as f32,
        pitch: pitch as f32,
        velocity: Vec3::ZERO,
    })
    .init_resource::<Terrain>()
    .insert_resource(Bench {
        last: now,
        start: now,
        stable: now,
        signature: (0, 0, 0),
        warm: 0,
        samples: vec![],
        cpu: cpu_time::ProcessTime::now(),
        empty: 0,
        geometry: 0,
        inputs: 0,
        baseline: (0, 0, 0.),
        shot: false,
        creature_ready_ms: None,
    })
    .insert_resource(ClearColor(Color::srgb_u8(123, 175, 210)))
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (
            control,
            creature_motion::update_demo,
            stream,
            network::update,
            daylight::sync,
            publish,
            creature_motion::publish_actors,
            measure,
            screenshot_key,
        )
            .chain(),
    );
    if profiling {
        app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
    }
    if measuring {
        app.init_resource::<AnimationTiming>().add_systems(
            PostUpdate,
            (
                begin_animation_timing.before(bevy::app::AnimationSystems),
                end_animation_timing.after(bevy::app::AnimationSystems),
            )
                .chain(),
        );
    }
    if creatures > 0 {
        app.add_plugins(creature_asset::CreatureAssetPlugin::new(manifest))
            .add_plugins(creature_motion::CreatureMotionPlugin)
            .add_systems(Update, (check_creature_assets, creature_shadow_flags));
    }
    app.run()
}

#[derive(Resource, Default)]
struct AnimationTiming {
    start: Option<Instant>,
    samples: Vec<f64>,
}
fn begin_animation_timing(mut timer: ResMut<AnimationTiming>, bench: Res<Bench>) {
    timer.start = (bench.warm >= 120).then(Instant::now);
}
fn end_animation_timing(mut timer: ResMut<AnimationTiming>) {
    if let Some(start) = timer.start.take() {
        timer.samples.push(start.elapsed().as_secs_f64() * 1e6);
    }
}
fn check_creature_assets(
    assets: Res<creature_asset::CreatureAssets>,
    mut exit: MessageWriter<AppExit>,
) {
    if let creature_asset::CreatureAssetStatus::Failed(message) = assets.status() {
        eprintln!("Créature non chargée : {message}");
        exit.write(AppExit::error());
    }
}

fn creature_shadow_flags(
    o: Res<Options>,
    new_parts: Query<Entity, Added<creature_asset::CreatureMesh>>,
    mut commands: Commands,
) {
    if !o.creature_shadows {
        for entity in &new_parts {
            commands.entity(entity).insert(bevy::light::NotShadowCaster);
        }
    }
}
fn setup(
    mut cmds: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut extended: ResMut<Assets<forest::ForestMaterial>>,
    o: Res<Options>,
    gpu: Res<RenderAdapterInfo>,
) {
    let name = gpu.name.to_lowercase();
    println!(
        "GPU={} backend={:?} type={:?}",
        gpu.name, gpu.backend, gpu.device_type
    );
    assert!(
        format!("{:?}", gpu.device_type) != "Cpu"
            && ![
                "warp",
                "llvmpipe",
                "swiftshader",
                "basic render",
                "software"
            ]
            .iter()
            .any(|s| name.contains(s)),
        "Rendu logiciel refusé"
    );
    let base = StandardMaterial {
        unlit: !o.style.lighting,
        cull_mode: o
            .style
            .cull
            .then_some(bevy::render::render_resource::Face::Back),
        ..default()
    };
    cmds.insert_resource(Surface {
        flora: materials.add(StandardMaterial {
            unlit: !o.style.lighting,
            cull_mode: None,
            double_sided: true,
            perceptual_roughness: 0.82,
            ..default()
        }),
        historic: materials.add(StandardMaterial {
            unlit: !o.pbr,
            cull_mode: None,
            ..default()
        }),
        forest: materials.add(base.clone()),
        grain: (o.style.palette && o.style.variation).then(|| {
            extended.add(forest::ForestMaterial {
                base,
                extension: default(),
            })
        }),
    });
    cmds.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 65f32.to_radians(),
            near: 0.05,
            far: (o.radius * 16 + 48) as f32,
            ..default()
        }),
        Msaa::Off,
        Tonemapping::None,
        bevy::core_pipeline::tonemapping::DebandDither::Disabled,
    ));
    if o.pbr {
        cmds.spawn((
            DirectionalLight {
                shadow_maps_enabled: false,
                illuminance: 10000.,
                ..default()
            },
            Transform::from_xyz(-2., 5., 3.).looking_at(Vec3::ZERO, Vec3::Y),
        ));
    }
}
#[allow(
    clippy::too_many_arguments,
    reason = "Bevy injects these independent system parameters"
)]
fn control(
    mut rig: ResMut<CameraRig>,
    o: Res<Options>,
    bench: Res<Bench>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    wheel: Res<bevy::input::mouse::AccumulatedMouseScroll>,
    time: Res<Time<Real>>,
    mut cursor: Single<&mut CursorOptions>,
) {
    let old = rig.eye;
    if o.frames == 0 {
        if buttons.just_pressed(MouseButton::Left) {
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        }
        if keys.just_pressed(KeyCode::Escape) {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
        if !cursor.visible {
            rig.yaw -= mouse.delta.x * 0.0025;
            rig.pitch = (rig.pitch - mouse.delta.y * 0.0025).clamp(-1.54, 1.54);
        }
        let axis = |a, b| u8::from(keys.pressed(a)) as f32 - u8::from(keys.pressed(b)) as f32;
        let f = rig.front();
        if o.creatures > 0 {
            rig.eye += f.as_dvec3() * f64::from(wheel.delta.y.clamp(-8., 8.)) * 0.75;
        }
        let delta = (f * axis(KeyCode::KeyW, KeyCode::KeyS)
            + f.cross(Vec3::Y).normalize_or_zero() * axis(KeyCode::KeyD, KeyCode::KeyA)
            + Vec3::Y * axis(KeyCode::Space, KeyCode::ControlLeft))
        .normalize_or_zero();
        rig.eye += (delta
            * time.delta_secs()
            * if keys.pressed(KeyCode::ShiftLeft) {
                60.
            } else {
                12.
            })
        .as_dvec3();
    } else if bench.warm >= 120 {
        let i = bench.samples.len() as f64;
        match o.route.as_str() {
            "forest-drift" => {
                let p = o.view.expect("forest-drift exige --view");
                let t = i / o.frames.saturating_sub(1).max(1) as f64;
                rig.eye = DVec3::new(p[0] + 3. * t, p[1] + 0.15 * t, p[2] + 1.75 * t);
                rig.yaw = (p[3] + 0.16 * t) as f32;
            }
            "drift" => {
                let t = i / (o.frames.saturating_sub(1).max(1) as f64);
                rig.eye = DVec3::new(0.5 + 4. * t, 18., 0.5 + 2. * t);
                rig.yaw = 0.78 + 0.10 * t as f32;
            }
            "forest-traverse" => {
                let x = i * 0.40;
                let z = 4. + i * 0.10;
                let ground = primal_voxel::surface_for(
                    primal_voxel::Preset::ForestModern,
                    20_261_120,
                    x as i32,
                    z as i32,
                );
                rig.eye = DVec3::new(x, ground as f64 + 9., z);
                rig.yaw = 1.31;
                rig.pitch = -0.12;
            }
            "traverse" => rig.eye = DVec3::new(0.5 + i * 0.8, 32., 0.5 + i * 0.2),
            "far" => {
                rig.eye = DVec3::new(
                    -1_500_000_000.375 + i * 0.125,
                    32.,
                    1_499_999_999.625 + i * 0.0625,
                )
            }
            _ => {}
        }
    }
    rig.velocity =
        ((rig.eye - old).as_vec3() / time.delta_secs().max(0.001)).clamp_length_max(100.);
}
fn stream(world: Option<ResMut<LocalWorld>>, mut terrain: ResMut<Terrain>, rig: Res<CameraRig>) {
    let Some(mut world) = world else { return };
    let s = world.stream.get_mut().unwrap();
    let start = Instant::now();
    let frame = s.tick(View {
        block: rig.eye.floor().as_ivec3().to_array(),
        forward: rig.front().to_array(),
        velocity: rig.velocity.to_array(),
    });
    terrain.stream_us += start.elapsed().as_secs_f64() * 1e6;
    terrain.stream_ticks += 1;
    assert!(frame.errors.is_empty(), "{:?}", frame.errors);
    for c in frame.evicted {
        terrain.remove(c);
    }
    for u in frame.uploads {
        if s.world().mesh_revision(u.chunk) == Some(u.revision) {
            terrain.install(u.chunk, u.payload);
        }
    }
}
#[allow(
    clippy::too_many_arguments,
    reason = "Bevy injects these independent system parameters"
)]
fn publish(
    mut terrain: ResMut<Terrain>,
    rig: Res<CameraRig>,
    o: Res<Options>,
    mut cmds: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    material: Res<Surface>,
    mut cam: Single<&mut Transform, With<Camera3d>>,
    window: Single<&Window>,
) {
    terrain.rebase(rig.eye, &mut cmds);
    terrain.publish(&mut cmds, &mut meshes, &material);
    let local = (rig.eye - DVec3::from_array(terrain.origin.map(|x| f64::from(x) * 16.))).as_vec3();
    let front = rig.front();
    **cam = Transform::from_translation(local).looking_to(front, Vec3::Y);
    terrain.visibility(
        rig.eye,
        front,
        (o.radius * 16) as f32,
        window.width() / window.height(),
        o.style.veg_distance,
        &mut cmds,
    );
}
#[allow(
    clippy::too_many_arguments,
    reason = "Bevy injects these independent system parameters"
)]
fn measure(
    mut bench: ResMut<Bench>,
    o: Res<Options>,
    terrain: Res<Terrain>,
    world: Option<Res<LocalWorld>>,
    online: Option<Res<network::Online>>,
    rig: Res<CameraRig>,
    gpu: Res<RenderAdapterInfo>,
    animation: Option<Res<AnimationTiming>>,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    assets: Option<Res<creature_asset::CreatureAssets>>,
    motion: Option<Res<creature_motion::MotionReport>>,
    mut cmds: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    if o.frames == 0 {
        return;
    }
    let now = Instant::now();
    let ms = (now - bench.last).as_secs_f64() * 1000.;
    bench.last = now;
    let ready = assets
        .as_ref()
        .is_none_or(|a| a.counts().players == o.creatures);
    if ready && assets.is_some() && bench.creature_ready_ms.is_none() {
        bench.creature_ready_ms = Some((now - bench.start).as_secs_f64() * 1000.);
    }
    let sig = (terrain.len(), terrain.uploads, terrain.removed);
    if bench.warm < 120 {
        if online
            .as_ref()
            .is_some_and(|n| !n.0.lock().unwrap().ready())
            || !ready
            || sig != bench.signature
            || !terrain.idle()
            || terrain.visible == 0
        {
            bench.signature = sig;
            bench.stable = now;
            bench.warm = 0;
        }
        if now - bench.stable > Duration::from_secs(1) {
            bench.warm += 1;
        }
        assert!(
            now - bench.start < Duration::from_secs(120),
            "Terrain non stabilisé"
        );
        if bench.warm == 120 {
            bench.cpu = cpu_time::ProcessTime::now();
            bench.inputs = online.as_ref().map_or(0, |n| n.0.lock().unwrap().inputs());
            bench.baseline = (terrain.uploads, terrain.bytes, terrain.publish_us);
        }
        return;
    }
    if !bench.shot
        && let Some(path) = &o.capture
    {
        cmds.spawn(Screenshot::primary_window())
            .observe(save_to_disk(path.clone()));
        bench.shot = true;
    }
    bench.empty += usize::from(terrain.visible == 0);
    bench.geometry =
        primal_net::playtest::geometry_digest(bench.geometry, terrain.visible, terrain.quads);
    bench.samples.push(ms);
    if bench.samples.len() < o.frames {
        return;
    }
    let mut sorted = bench.samples.clone();
    sorted.sort_by(f64::total_cmp);
    let pct = |p: f64| sorted[(p * sorted.len() as f64).ceil() as usize - 1];
    let tail = &sorted[sorted.len() - (0.01 * sorted.len() as f64).ceil() as usize..];
    let stream = world.as_ref().map(|w| w.stream.lock().unwrap());
    let stats = stream.as_ref().map(|s| s.stats());
    let mut report = json!({"renderer":"bevy","adapter":gpu.name,"backend":format!("{:?}",gpu.backend),"device_type":format!("{:?}",gpu.device_type),
        "material":if o.pbr{"pbr"}else{"simple"},"route":o.route,"resolution":o.size,"render_radius":o.radius,"frames":sorted.len(),
        "geometry_digest":format!("{:016x}",bench.geometry),"sent_inputs":online.as_ref().map(|n|n.0.lock().unwrap().inputs()-bench.inputs),
        "process_cpu_ms":bench.cpu.elapsed().as_secs_f64()*1000.,"players":online.as_ref().map(|n|n.0.lock().unwrap().player_count),
        "fps_avg":1000.*sorted.len() as f64/sorted.iter().sum::<f64>(),"fps_1low":1000.*tail.len() as f64/tail.iter().sum::<f64>(),
        "p95_ms":pct(0.95),"p99_ms":pct(0.99),"p999_ms":pct(0.999),"empty_frames":bench.empty,"visible_chunks":terrain.visible,
        "quads":terrain.quads,"triangles":terrain.quads*2,"mesh_publications":terrain.uploads-bench.baseline.0,
        "published_bytes":terrain.bytes-bench.baseline.1,"publication_cpu_us":terrain.publish_us-bench.baseline.2,
        "peak_meshes":terrain.peak,"peak_resident":stats.map(|s|s.peak_resident),"peak_jobs":stats.map(|s|s.peak_jobs),"evictions":stats.map(|s|s.evictions),
        "meshes_retired":terrain.removed,"rebases":terrain.rebases,"final_eye":rig.eye.to_array()});
    if o.preset == Some(primal_voxel::Preset::ForestModern) {
        report["material"] = json!(if o.style.lighting {
            "forest_lit"
        } else {
            "forest_unlit"
        });
        report["forest"] = json!({
            "decorative_triangles":terrain.flora_tri, "plants":terrain.plants,
            "visible_triangles_total":terrain.quads*2 + terrain.flora_tri,
            "cumulative_upload_bytes":terrain.bytes, "cumulative_meshes":terrain.uploads,
            "cumulative_build_us":terrain.build_us, "completed_builds":terrain.built,
            "cumulative_stream_us":terrain.stream_us,"stream_ticks":terrain.stream_ticks,
            "frame_ms":bench.samples
        });
    }
    if let Some(a) = assets {
        report["creatures"] = json!(a.counts());
        report["creature_import"] = json!(a.info());
        report["app_start_to_rig_ready_ms"] = json!(bench.creature_ready_ms);
    }
    if let Some(motion) = motion {
        report["creature_motion"] = motion.json();
    }
    report["driver"] = json!([gpu.driver, gpu.driver_info]);
    if let Some(timer) = animation {
        let mean = timer.samples.iter().sum::<f64>() / timer.samples.len().max(1) as f64;
        report["animation_schedule_us"] = json!({"mean":mean,"samples":timer.samples});
    }
    if o.creature_profile {
        report["render_passes"] = serde_json::Value::Object(
            diagnostics
                .iter()
                .filter_map(|d| {
                    let name = d.path().to_string();
                    (name.starts_with("render/")
                        && (name.ends_with("elapsed_cpu") || name.ends_with("elapsed_gpu")))
                    .then(|| d.average().map(|v| (name, json!(v))))
                    .flatten()
                })
                .collect(),
        );
    }
    if let Some(path) = &o.report {
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    println!("{report}");
    exit.write(AppExit::Success);
}

fn screenshot_key(
    o: Res<Options>,
    keys: Res<ButtonInput<KeyCode>>,
    mut cmds: Commands,
    rig: Res<CameraRig>,
) {
    if o.frames == 0
        && keys.just_pressed(KeyCode::F12)
        && let Some(path) = &o.capture
    {
        println!(
            "Capture eye={:?} forward={:?}",
            rig.eye.to_array(),
            rig.front().to_array()
        );
        cmds.spawn(Screenshot::primary_window())
            .observe(save_to_disk(path.clone()));
    }
}
