use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::time::Duration;

/// Benchmarks keep samples in RAM until the last frame, outside measured disk I/O.
pub struct Profile(Option<Vec<(usize, &'static str, Duration)>>);
impl Profile {
    pub fn new(enabled: bool) -> Self {
        Self(enabled.then(Vec::new))
    }
    pub fn sample(&mut self, frame: usize, stage: &'static str, elapsed: Duration) {
        if let Some(samples) = &mut self.0 {
            samples.push((frame, stage, elapsed));
        }
    }
    pub fn write(&self, path: &Path) -> io::Result<()> {
        let mut out = BufWriter::new(std::fs::File::create(path)?);
        writeln!(out, "frame,stage,ns")?;
        for (frame, stage, elapsed) in self.0.iter().flatten() {
            writeln!(out, "{frame},{stage},{}", elapsed.as_nanos())?;
        }
        out.flush()
    }
}
