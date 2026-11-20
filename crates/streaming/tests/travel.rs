use primal_storage::RegionStore;
use primal_streaming::{MeshKind, StreamConfig, Streamer, View};
use primal_voxel::{World, chunk_at};
use std::time::{Duration, Instant};

#[test]
fn repeated_kilometres_bound_residency_and_reopen_a_distant_edit() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("travel.pvx");
    let store = RegionStore::open(&path, 42).unwrap();
    let config = StreamConfig {
        load_radius: 1,
        render_radius: 1,
        simulation_radius: 0,
        preload_radius: 2,
        max_chunks: 12,
        workers: 3,
        max_jobs: 8,
        uploads_per_tick: 2,
        ..Default::default()
    };
    let mut stream = Streamer::new(
        World::streamed(42),
        store,
        config,
        MeshKind::Greedy,
        |faces| (faces.len(), faces.len() * 8),
    )
    .unwrap();
    let start = Instant::now();
    let mut positions: Vec<_> = (-160..=160).map(|i| i * 16).collect();
    positions.extend_from_slice(&[1_000_000, -1_000_000, 1_000_000]);
    let edit = [1_000_000, 112, -544];
    let mut wrote = false;
    for x in positions {
        let p = [x, 112, -544];
        let view = View {
            block: p,
            forward: [1., 0., 0.],
            velocity: [160., 0., 0.],
        };
        loop {
            let frame = stream.tick(view);
            assert!(frame.errors.is_empty(), "{:?}", frame.errors);
            assert!(frame.uploads.len() <= 2);
            assert!(stream.world().resident_len() <= 12);
            assert!(stream.stats().peak_jobs <= 8);
            if stream.world().is_resident(chunk_at(p))
                && stream.visible_revision(chunk_at(p)).is_some()
            {
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(90));
            std::thread::yield_now();
        }
        if x == 1_000_000 {
            if wrote {
                assert_eq!(stream.world().lookup(edit), Some(7));
            } else {
                assert!(stream.world_mut().set(edit, 7));
                wrote = true;
            }
        }
    }
    println!(
        "checkpoints=324 continuous_blocks=5120 far_x=1000000 elapsed_ms={} {:?}",
        start.elapsed().as_millis(),
        stream.stats()
    );
    drop(stream);
    let reopened = RegionStore::open(&path, 0).unwrap();
    assert_eq!(reopened.load_chunk(chunk_at(edit)).unwrap(), [(edit, 7)]);
}
