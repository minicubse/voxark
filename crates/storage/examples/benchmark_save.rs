//! Release benchmark. Load includes terrain regeneration. Verification is outside the timers.
use primal_storage::{load_or_create, save};
use primal_voxel::World;
use std::io;
use std::time::Instant;

fn main() -> io::Result<()> {
    let scratch = tempfile::tempdir()?;
    for count in [1_usize, 10_000, 65_536] {
        let mut world = World::new(42);
        for i in 0..count {
            let pos = [
                (i % 128) as i32 - 64,
                (i / 16_384) as i32,
                ((i / 128) % 128) as i32 - 64,
            ];
            // Every operation changes a real cell, regardless of generator composition.
            let replacement = if world.get(pos) == 0 { 3 } else { 0 };
            assert!(world.set(pos, replacement));
        }
        let path = scratch.path().join(format!("{count}.pvx"));
        let expected = world.edits();
        save(&path, &world)?;
        let mut saves = Vec::new();
        let mut loads = Vec::new();
        for _ in 0..11 {
            let start = Instant::now();
            save(&path, &world)?;
            saves.push(start.elapsed().as_secs_f64() * 1000.0);
            let start = Instant::now();
            let loaded = load_or_create(&path, 0)?;
            loads.push(start.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(loaded.seed(), 42);
            assert_eq!(loaded.edits(), expected);
        }
        saves.sort_by(f64::total_cmp);
        loads.sort_by(f64::total_cmp);
        println!(
            "edits={count} bytes={} samples=11 save_median_ms={:.3} save_max_ms={:.3} load_median_ms={:.3} load_max_ms={:.3}",
            path.metadata()?.len(),
            saves[5],
            saves[10],
            loads[5],
            loads[10]
        );
    }
    Ok(())
}
