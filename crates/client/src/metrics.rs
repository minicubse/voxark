use std::path::Path;

#[derive(Default)]
pub struct FrameMetrics {
    samples_ms: Vec<f64>,
}

impl FrameMetrics {
    pub fn record(&mut self, frame_seconds: f32) {
        if frame_seconds.is_finite() && frame_seconds > 0.0 {
            self.samples_ms.push(f64::from(frame_seconds) * 1000.0);
        }
    }

    pub fn json(
        &self,
        width: i32,
        height: i32,
        chunks: usize,
        faces: usize,
        mesher: &str,
    ) -> String {
        let mut samples = self.samples_ms.clone();
        samples.sort_by(f64::total_cmp);
        let mean = samples.iter().sum::<f64>() / samples.len().max(1) as f64;
        let low_one = slowest_fps(&samples, 0.01);
        let low_point_one = slowest_fps(&samples, 0.001);
        format!(
            "{{\n  \"resolution\": [{width}, {height}],\n  \"mesher\": \"{mesher}\",\n  \"samples\": {},\n  \"warmup_frames\": 120,\n  \"frame_mean_ms\": {mean:.4},\n  \"fps_mean\": {:.3},\n  \"fps_1_percent_low\": {low_one:.3},\n  \"fps_0_1_percent_low\": {low_point_one:.3},\n  \"frame_p50_ms\": {:.4},\n  \"frame_p95_ms\": {:.4},\n  \"frame_p99_ms\": {:.4},\n  \"frame_p99_9_ms\": {:.4},\n  \"frame_max_ms\": {:.4},\n  \"mesh_chunks\": {chunks},\n  \"render_quads\": {faces},\n  \"method\": \"macroquad frame interval including presentation; deterministic camera route; 120 warmup frames; percentile nearest rank; lows reciprocal mean slowest fraction\"\n}}\n",
            samples.len(),
            if mean > 0.0 { 1000.0 / mean } else { 0.0 },
            percentile(&samples, 0.50),
            percentile(&samples, 0.95),
            percentile(&samples, 0.99),
            percentile(&samples, 0.999),
            samples.last().copied().unwrap_or(0.0),
        )
    }

    pub fn save(
        &self,
        path: &Path,
        width: i32,
        height: i32,
        chunks: usize,
        faces: usize,
        mesher: &str,
    ) -> std::io::Result<()> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.json(width, height, chunks, faces, mesher))
    }
}

fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((sorted.len() as f64 * fraction).ceil() as usize).max(1);
    sorted[rank.min(sorted.len()) - 1]
}

fn slowest_fps(sorted: &[f64], fraction: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let count = ((sorted.len() as f64 * fraction).ceil() as usize).max(1);
    let slowest = &sorted[sorted.len() - count..];
    1000.0 / (slowest.iter().sum::<f64>() / count as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_nearest_rank_and_slowest_fraction() {
        let samples: Vec<_> = (1..=100).map(f64::from).collect();
        assert_eq!(percentile(&samples, 0.95), 95.0);
        assert_eq!(percentile(&samples, 0.99), 99.0);
        assert_eq!(slowest_fps(&samples, 0.01), 10.0);
        assert_eq!(slowest_fps(&samples, 0.02), 1000.0 / 99.5);
    }

    #[test]
    fn rejects_nonfinite_samples_and_handles_empty_metrics() {
        let mut metrics = FrameMetrics::default();
        metrics.record(f32::NAN);
        metrics.record(-1.0);
        assert!(
            metrics
                .json(1280, 720, 0, 0, "faces")
                .contains("\"samples\": 0")
        );
        assert!(!metrics.json(1280, 720, 0, 0, "faces").contains("NaN"));
    }
}
