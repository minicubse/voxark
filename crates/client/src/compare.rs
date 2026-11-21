//! Renderer comparison with a fixed camera, no overlays and uncapped presentation.
use crate::{buffers::Buffers, metrics::FrameMetrics, options::Options, terrain::Terrain};
use macroquad::prelude::*;
use primal_streaming::{MeshKind, Streamer, View};
use std::time::{Duration, Instant};

pub(super) fn camera(drift: bool, index: usize, count: usize) -> (Vec3, Vec3) {
    let t = if drift {
        index as f32 / count.saturating_sub(1).max(1) as f32
    } else {
        0.
    };
    let eye = vec3(0.5 + 4. * t, 18., 0.5 + 2. * t);
    (
        eye,
        Vec3::from_array(primal_net::direction(0.78 + 0.10 * t, -0.32)),
    )
}
pub(super) fn draw(terrain: &Terrain, eye: Vec3, front: Vec3, radius: i32) {
    clear_background(Color::from_rgba(123, 175, 210, 255));
    set_camera(&Camera3D {
        position: eye,
        target: eye + front,
        up: Vec3::Y,
        fovy: crate::terrain::VERTICAL_FOV,
        z_near: 0.05,
        z_far: (radius * 16 + 48) as f32,
        ..Default::default()
    });
    terrain.draw(eye, front, (radius * 16) as f32, true);
    set_default_camera();
}
pub(super) async fn local(o: &Options) -> Result<(), String> {
    let store =
        primal_storage::RegionStore::open(&o.world, crate::SEED).map_err(|e| e.to_string())?;
    let mut cfg = o.stream.clone();
    cfg.uploads_per_tick = 6;
    let mut stream = Streamer::new(
        primal_voxel::World::streamed(crate::SEED),
        store,
        cfg,
        MeshKind::Greedy,
        Buffers::prepare,
    )
    .map_err(|e| e.to_string())?;
    let mut terrain = Terrain::default();
    let mut samples = FrameMetrics::default();
    let mut i = 0;
    let mut warm = 0;
    let mut stable = Instant::now();
    let start = Instant::now();
    let mut prev = start;
    loop {
        let (eye, front) = camera(o.compare_drift, i, o.frames);
        let f = stream.tick(View {
            block: eye.floor().as_ivec3().to_array(),
            forward: front.to_array(),
            velocity: [0.; 3],
        });
        if let Some(e) = f.errors.into_iter().next() {
            return Err(e);
        }
        if warm < 120 && (!f.uploads.is_empty() || !f.evicted.is_empty()) {
            stable = Instant::now();
            warm = 0;
        }
        for c in f.evicted {
            terrain.remove(c);
        }
        for u in f.uploads {
            if stream.world().mesh_revision(u.chunk) == Some(u.revision) {
                terrain.install(u.chunk, u.payload);
            }
        }
        draw(&terrain, eye, front, o.stream.render_radius);
        next_frame().await;
        let now = Instant::now();
        if warm >= 120 {
            samples.record((now - prev).as_secs_f32());
            i += 1;
        } else if terrain.last_draw.get().chunks > 0 && stable.elapsed() > Duration::from_secs(1) {
            warm += 1;
        }
        prev = now;
        if i == o.frames {
            break;
        }
        if start.elapsed() > Duration::from_secs(120) {
            return Err("Terrain non stabilisé".into());
        }
    }
    let d = terrain.last_draw.get();
    let s = samples.json(o.width, o.height, d.chunks, d.quads, "greedy");
    if let Some(path) = &o.metrics {
        std::fs::write(path, &s).map_err(|e| e.to_string())?;
    }
    println!("{s}");
    Ok(())
}
