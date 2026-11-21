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
    world: Option<PathBuf>,
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
            world: None,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            let val = args
                .next()
                .unwrap_or_else(|| panic!("Valeur manquante pour {arg}"));
            match arg.as_str() {
                "--resolution" => {
                    let (w, h) = val.split_once('x').expect("WIDTHxHEIGHT");
                    o.size = (w.parse().unwrap(), h.parse().unwrap());
                }
                "--render-radius" => o.radius = val.parse().unwrap(),
                "--frames" | "--net-frames" => o.frames = val.parse().unwrap(),
                "--report" | "--net-report" => o.report = Some(val.into()),
                "--capture" => o.capture = Some(val.into()),
                "--world" => o.world = Some(val.into()),
                "--route" | "--compare-route" => {
                    assert!(matches!(
                        val.as_str(),
                        "fixed" | "drift" | "traverse" | "far"
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
        assert!((1..=23).contains(&o.radius));
        o
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
struct Surface(Handle<StandardMaterial>);
#[derive(Resource)]
struct Bench {
    last: Instant,
    start: Instant,
    stable: Instant,
    signature: (usize, usize, usize),
    warm: usize,
    samples: Vec<f64>,
    empty: usize,
    baseline: (usize, usize, f64),
    shot: bool,
}
fn main() -> AppExit {
    let o = Options::parse();
    let dir = o.world.is_none().then(|| tempfile::tempdir().unwrap());
    let path = o
        .world
        .clone()
        .unwrap_or_else(|| dir.as_ref().unwrap().path().join("world.pvx"));
    let store = RegionStore::open(&path, 20_261_120).expect("ouverture du monde");
    let cfg = StreamConfig {
        load_radius: o.radius + 1,
        render_radius: o.radius,
        uploads_per_tick: 6,
        ..default()
    };
    let local_stream = Streamer::new(
        primal_voxel::World::streamed(store.seed()),
        store,
        cfg,
        MeshKind::GreedyLocal,
        terrain::prepare,
    )
    .unwrap();
    let eye = if o.route == "far" {
        DVec3::new(-1_500_000_000.375, 32., 1_499_999_999.625)
    } else {
        DVec3::new(0.5, 18., 0.5)
    };
    let now = Instant::now();
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Primal Voxel — Bevy".into(),
                resolution:
                    WindowResolution::new(o.size.0, o.size.1).with_scale_factor_override(1.),
                present_mode: PresentMode::Immediate,
                ..default()
            }),
            ..default()
        }))
        .insert_resource(WinitSettings::continuous())
        .insert_resource(o)
        .insert_resource(CameraRig {
            eye,
            yaw: 0.78,
            pitch: -0.32,
            velocity: Vec3::ZERO,
        })
        .insert_resource(LocalWorld {
            stream: Mutex::new(local_stream),
            _dir: dir,
        })
        .init_resource::<Terrain>()
        .insert_resource(Bench {
            last: now,
            start: now,
            stable: now,
            signature: (0, 0, 0),
            warm: 0,
            samples: vec![],
            empty: 0,
            baseline: (0, 0, 0.),
            shot: false,
        })
        .insert_resource(ClearColor(Color::srgb_u8(123, 175, 210)))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (control, stream, publish, measure, screenshot_key).chain(),
        )
        .run()
}
fn setup(
    mut cmds: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
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
    cmds.insert_resource(Surface(materials.add(StandardMaterial {
        unlit: !o.pbr,
        cull_mode: None,
        ..default()
    })));
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
fn control(
    mut rig: ResMut<CameraRig>,
    o: Res<Options>,
    bench: Res<Bench>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    time: Res<Time>,
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
            "drift" => {
                let t = i / (o.frames.saturating_sub(1).max(1) as f64);
                rig.eye = DVec3::new(0.5 + 4. * t, 18., 0.5 + 2. * t);
                rig.yaw = 0.78 + 0.10 * t as f32;
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
fn stream(mut world: ResMut<LocalWorld>, mut terrain: ResMut<Terrain>, rig: Res<CameraRig>) {
    let s = world.stream.get_mut().unwrap();
    let frame = s.tick(View {
        block: rig.eye.floor().as_ivec3().to_array(),
        forward: rig.front().to_array(),
        velocity: rig.velocity.to_array(),
    });
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
    terrain.publish(&mut cmds, &mut meshes, &material.0);
    let local = (rig.eye - DVec3::from_array(terrain.origin.map(|x| f64::from(x) * 16.))).as_vec3();
    let front = rig.front();
    **cam = Transform::from_translation(local).looking_to(front, Vec3::Y);
    terrain.visibility(
        rig.eye,
        front,
        (o.radius * 16) as f32,
        window.width() / window.height(),
        &mut cmds,
    );
}
fn measure(
    mut bench: ResMut<Bench>,
    o: Res<Options>,
    terrain: Res<Terrain>,
    world: Res<LocalWorld>,
    rig: Res<CameraRig>,
    gpu: Res<RenderAdapterInfo>,
    mut cmds: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    if o.frames == 0 {
        return;
    }
    let now = Instant::now();
    let ms = (now - bench.last).as_secs_f64() * 1000.;
    bench.last = now;
    let sig = (terrain.len(), terrain.uploads, terrain.removed);
    if bench.warm < 120 {
        if sig != bench.signature || !terrain.idle() || terrain.visible == 0 {
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
    bench.samples.push(ms);
    if bench.samples.len() < o.frames {
        return;
    }
    let mut sorted = bench.samples.clone();
    sorted.sort_by(f64::total_cmp);
    let pct = |p: f64| sorted[(p * sorted.len() as f64).ceil() as usize - 1];
    let tail = &sorted[sorted.len() - (0.01 * sorted.len() as f64).ceil() as usize..];
    let s = world.stream.lock().unwrap();
    let stats = s.stats();
    let report = json!({"renderer":"bevy","adapter":gpu.name,"backend":format!("{:?}",gpu.backend),"device_type":format!("{:?}",gpu.device_type),
        "material":if o.pbr{"pbr"}else{"simple"},"route":o.route,"resolution":o.size,"render_radius":o.radius,"frames":sorted.len(),
        "fps_avg":1000.*sorted.len() as f64/sorted.iter().sum::<f64>(),"fps_1low":1000.*tail.len() as f64/tail.iter().sum::<f64>(),
        "p95_ms":pct(0.95),"p99_ms":pct(0.99),"p999_ms":pct(0.999),"empty_frames":bench.empty,"visible_chunks":terrain.visible,
        "quads":terrain.quads,"triangles":terrain.quads*2,"mesh_publications":terrain.uploads-bench.baseline.0,
        "published_bytes":terrain.bytes-bench.baseline.1,"publication_cpu_us":terrain.publish_us-bench.baseline.2,
        "peak_meshes":terrain.peak,"peak_resident":stats.peak_resident,"peak_jobs":stats.peak_jobs,"evictions":stats.evictions,
        "meshes_retired":terrain.removed,"rebases":terrain.rebases,"final_eye":rig.eye.to_array()});
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
