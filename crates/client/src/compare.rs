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
        primal_voxel::World::with_preset(store.seed(), store.preset()),
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

pub(super) struct NetworkBench {
    metrics: FrameMetrics,
    stable: Instant,
    previous: Instant,
    cpu: cpu_time::ProcessTime,
    warm: usize,
    frames: usize,
    empty: usize,
    draw_us: f64,
    geometry: u64,
    inputs: u64,
    bytes: u64,
}
impl NetworkBench {
    pub fn new() -> Self {
        Self {
            metrics: FrameMetrics::default(),
            stable: Instant::now(),
            previous: Instant::now(),
            cpu: cpu_time::ProcessTime::now(),
            warm: 0,
            frames: 0,
            empty: 0,
            draw_us: 0.,
            geometry: 0,
            inputs: 0,
            bytes: 0,
        }
    }
    pub async fn frame(
        &mut self,
        o: &Options,
        terrain: &Terrain,
        ready: bool,
        sent_inputs: u64,
    ) -> Result<bool, String> {
        let count = o.net_frames.unwrap_or(480);
        let (eye, front) = camera(o.compare_drift, self.frames, count);
        let draw_start = Instant::now();
        draw(terrain, eye, front, o.stream.render_radius);
        let draw_us = draw_start.elapsed().as_secs_f64() * 1e6;
        let stats = terrain.last_draw.get();
        next_frame().await;
        let now = Instant::now();
        let dt = now - self.previous;
        self.previous = now;
        if self.warm < 120 {
            if !ready || stats.chunks == 0 {
                self.stable = now;
                self.warm = 0;
            }
            if now - self.stable > Duration::from_secs(1) {
                self.warm += 1;
            }
            if self.warm == 120 {
                self.cpu = cpu_time::ProcessTime::now();
                self.inputs = sent_inputs;
            }
            return Ok(false);
        }
        self.frames += 1;
        self.empty += usize::from(stats.chunks == 0);
        self.draw_us += draw_us;
        self.bytes += stats.bytes as u64;
        self.geometry =
            primal_net::playtest::geometry_digest(self.geometry, stats.chunks, stats.quads);
        self.metrics.record(dt.as_secs_f32());
        if self.frames < count {
            return Ok(false);
        }
        let cpu_ms = self.cpu.elapsed().as_secs_f64() * 1000.;
        // Capture one additional frame. Telemetry draws thumbnails, so its cost is excluded.
        macroquad::telemetry::capture_frame();
        next_frame().await;
        draw(terrain, eye, front, o.stream.render_radius);
        next_frame().await;
        let calls = macroquad::telemetry::drawcalls();
        let indices = calls.iter().map(|c| c.indices_count).sum::<usize>();
        if indices != stats.quads * 6 {
            return Err(format!(
                "Capture draws {indices} indices pour {} quads",
                stats.quads
            ));
        }
        let mut report: serde_json::Value = serde_json::from_str(&self.metrics.json(
            o.width,
            o.height,
            stats.chunks,
            stats.quads,
            "greedy",
        ))
        .map_err(|e| e.to_string())?;
        let extra = serde_json::json!({"renderer":"macroquad","backend":"OpenGL","players":1,"render_radius":o.stream.render_radius,
            "geometry_digest":format!("{:016x}",self.geometry),"sent_inputs":sent_inputs-self.inputs,"route":if o.compare_drift{"drift"}else{"fixed"},"visible_chunks":stats.chunks,"quads":stats.quads,"triangles":stats.quads*2,
            "empty_frames":self.empty,"draws_audit_frame":calls.len(),"terrain_submissions":stats.submissions,"uploads_audit_frame":2*calls.len(),
            "vertex_index_bytes_all_frames":self.bytes,"cpu_draw_and_flush_us":self.draw_us,"process_cpu_ms":cpu_ms});
        report
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        if let Some(path) = &o.net_report {
            std::fs::write(path, report.to_string()).map_err(|e| e.to_string())?;
        }
        println!("{report}");
        Ok(true)
    }
}
