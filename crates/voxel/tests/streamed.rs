use primal_voxel::{ChunkPos, World, chunk_at, generate_chunk};

fn load(world: &mut World, chunk: ChunkPos) {
    let ticket = world.begin_load(chunk).unwrap();
    let data = generate_chunk(world.seed(), chunk).unwrap();
    assert!(world.install_chunk(ticket, data, &[]));
}

#[test]
fn streamed_addressing_and_deterministic_distant_chunks() {
    let mut world = World::streamed(42);
    assert_eq!(world.resident_len(), 0);
    assert_eq!(world.lookup([-1, 127, 16]), None);
    assert_eq!(chunk_at([-1, 127, 16]), [-1, 7, 1]);
    assert_eq!(chunk_at([-16, 0, -17]), [-1, 0, -2]);
    assert!(!world.set([-1, 127, 16], 7));
    for pos in [
        [-1, 127, 16],
        [1_000_003, 25, -987_654],
        [i32::MAX, 20, i32::MIN],
    ] {
        let chunk = chunk_at(pos);
        let expected = generate_chunk(42, chunk).unwrap().get(pos).unwrap();
        load(&mut world, chunk);
        assert_eq!(world.lookup(pos), Some(expected));
        assert!(world.evict_chunk(chunk));
        assert_eq!(world.lookup(pos), None);
        load(&mut world, chunk);
        assert_eq!(world.lookup(pos), Some(expected));
        assert!(world.evict_chunk(chunk));
    }
    assert!(world.begin_load([0, 8, 0]).is_none());
    assert!(generate_chunk(42, [i32::MAX, 0, 0]).is_none());
}

#[test]
fn cancelled_generation_cannot_replace_newer_edit_or_resurrect_evicted_chunk() {
    let mut world = World::streamed(17);
    let c = [-124, 7, 160];
    let p = [-1984, 120, 2561];
    let old = world.begin_load(c).unwrap();
    let late = std::thread::spawn(move || generate_chunk(17, c).unwrap());
    assert!(world.begin_load(c).is_none());
    assert!(world.cancel_load(old));
    load(&mut world, c);
    assert!(world.set(p, 7));
    assert!(!world.install_chunk(old, late.join().unwrap(), &[]));
    assert_eq!(world.lookup(p), Some(7));
    assert!(world.chunk_dirty(c));
    assert!(!world.evict_chunk(c));
    let saved = world.chunk_revision(c).unwrap();
    let first_snapshot = world.chunk_edits(c);
    assert_eq!(first_snapshot, [(p, 7)]);
    assert!(world.set(p, 0));
    assert!(!world.mark_saved(c, saved));
    assert!(world.chunk_edits(c).is_empty());
    assert!(!world.evict_chunk(c));
    assert!(world.mark_saved(c, world.chunk_revision(c).unwrap()));
    assert!(world.evict_chunk(c));
    assert!(!world.install_chunk(old, generate_chunk(17, c).unwrap(), &[]));
    assert_eq!(world.resident_len(), 0);
}

#[test]
fn loaded_edits_are_validated_atomically_and_restore_the_generated_original() {
    let mut world = World::streamed(1);
    let c = [15, 7, -15];
    let p = [243, 120, -235];
    for bad in [
        vec![(p, 7), (p, 7)],
        vec![(p, 0)],
        vec![(p, 8)],
        vec![([0, 0, 0], 7)],
    ] {
        let t = world.begin_load(c).unwrap();
        assert!(!world.install_chunk(t, generate_chunk(1, c).unwrap(), &bad));
        assert!(!world.is_resident(c));
    }
    let t = world.begin_load(c).unwrap();
    assert!(world.install_chunk(t, generate_chunk(1, c).unwrap(), &[(p, 7)]));
    assert!(!world.chunk_dirty(c));
    assert_eq!(world.chunk_edits(c), [(p, 7)]);
    assert!(world.set(p, 0));
    assert!(world.chunk_edits(c).is_empty());
    assert!(world.chunk_dirty(c));
}

#[test]
fn mesh_snapshot_observes_face_neighbour_lineage_and_immutable_occupancy() {
    let mut world = World::streamed(99);
    let left = [-33, 7, 18];
    let right = [-32, 7, 18];
    load(&mut world, left);
    assert!(world.set([-513, 120, 290], 7));
    let solitary = world.mesh_snapshot(left).unwrap();
    assert_eq!(solitary.greedy_mesh().len(), 6);
    load(&mut world, right);
    let arrival = world.mesh_revision(left).unwrap();
    assert_ne!(solitary.revision(), arrival);
    assert!(world.set([-511, 120, 290], 7));
    assert_eq!(Some(arrival), world.mesh_revision(left));
    let before_edit = world.mesh_snapshot(left).unwrap();
    assert!(world.set([-512, 120, 290], 7));
    assert_ne!(Some(before_edit.revision()), world.mesh_revision(left));
    assert_eq!(before_edit.mesh().len(), 6);
    assert_eq!(world.mesh(left).len(), 5);
    assert_eq!(world.greedy_mesh(left).len(), 5);
    let before_eviction = world.mesh_revision(left);
    assert!(world.mark_saved(right, world.chunk_revision(right).unwrap()));
    assert!(world.evict_chunk(right));
    assert_ne!(before_eviction, world.mesh_revision(left));
    assert_eq!(world.mesh(left).len(), 6);
}
