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

#[test]
fn distant_authorities_share_capacity_and_commit_before_last_interest_leaves() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("pair.pvx");
    let store = RegionStore::open(&path, 617).unwrap();
    let config = StreamConfig {
        max_chunks: 16,
        load_radius: 1,
        render_radius: 1,
        simulation_radius: 1,
        workers: 2,
        max_jobs: 4,
        uploads_per_tick: 1,
        ..Default::default()
    };
    let mut s = Streamer::new(
        World::streamed(617),
        store,
        config,
        MeshKind::Greedy,
        |_| ((), 0),
    )
    .unwrap();
    let a = View {
        block: [-1_000_000, 120, 2],
        forward: [0.; 3],
        velocity: [0.; 3],
    };
    let b = View {
        block: [900_000, 120, 2],
        ..a
    };
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut edits = false;
    loop {
        let positions: &[View] = if edits { &[] } else { &[a, b] };
        let f = s.tick_views(positions);
        assert!(f.errors.is_empty());
        assert!(s.world().resident_len() <= 16 && s.stats().peak_jobs <= 4);
        if !edits
            && [a, b]
                .iter()
                .all(|v| s.visible_revision(chunk_at(v.block)).is_some())
        {
            for v in [a, b] {
                assert!(s.world_mut().set(v.block, 7));
            }
            edits = true;
        } else if edits && s.world().resident_len() == 0 {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    drop(s);
    let disk = RegionStore::open(&path, 0).unwrap();
    for v in [a, b] {
        assert_eq!(disk.load_chunk(chunk_at(v.block)).unwrap(), [(v.block, 7)]);
    }
}

#[test]
fn background_chunk_becomes_visible_during_continuous_nearby_edits() {
    let temp = tempfile::tempdir().unwrap();
    let store = RegionStore::open(&temp.path().join("fair.pvx"), 17).unwrap();
    let config = StreamConfig {
        load_radius: 1,
        render_radius: 1,
        simulation_radius: 0,
        preload_radius: 0,
        max_chunks: 72,
        workers: 2,
        max_jobs: 4,
        uploads_per_tick: 1,
        ..Default::default()
    };
    let mut stream = Streamer::new(World::streamed(17), store, config, MeshKind::Greedy, |_| {
        ((), 0)
    })
    .unwrap();
    let view = View {
        block: [8, 120, 8],
        forward: [0.; 3],
        velocity: [0.; 3],
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut edits = 0;
    loop {
        let frame = stream.tick(view);
        assert!(frame.errors.is_empty());
        if stream.world().is_resident(chunk_at(view.block)) {
            assert!(stream.world_mut().set(view.block, 1 + (edits % 2) as u8));
            edits += 1;
        }
        if stream.visible_revision([-1, 0, -1]).is_some() {
            assert!(edits > 0);
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}
