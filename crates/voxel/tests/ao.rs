use primal_voxel::{ChunkPos, Face, MeshSnapshot, World, generate_chunk};

fn load(world: &mut World, chunk: ChunkPos, edits: &[([i32; 3], u8)]) {
    let ticket = world.begin_load(chunk).unwrap();
    let base = generate_chunk(world.seed(), chunk).unwrap();
    assert!(world.install_chunk(ticket, base, edits));
}

#[test]
fn corner_neighbour_invalidates_mesh_on_arrival_edit_and_eviction() {
    let mut world = World::streamed(99);
    let center = [0, 6, 0];
    let corner = [1, 7, 1];
    let block = [16, 112, 16];
    load(&mut world, center, &[]);
    let missing = world.mesh_revision(center).unwrap();

    load(&mut world, corner, &[]);
    let installed = world.mesh_revision(center).unwrap();
    assert!(installed > missing);
    assert!(world.set([17, 112, 16], 3));
    assert_eq!(world.mesh_revision(center), Some(installed));
    assert!(world.set(block, 3));
    let edited = world.mesh_revision(center).unwrap();
    assert!(edited > installed);
    assert!(world.dirty_chunks(block).contains(&center));

    assert!(world.mark_saved(corner, world.chunk_revision(corner).unwrap()));
    assert!(world.evict_chunk(corner));
    assert!(world.mesh_revision(center).unwrap() > edited);
}

fn tops(snapshot: &MeshSnapshot, local_y: f32) -> Vec<(Face, [u8; 4])> {
    snapshot
        .greedy_ao_local()
        .into_iter()
        .filter(|(face, _)| face.normal == [0, 1, 0] && face.corners[0][1] == local_y)
        .collect()
}

#[test]
fn ao_samples_corner_and_edge_chunks_and_keeps_old_snapshots_immutable() {
    let mut world = World::streamed(99);
    let center = [0, 6, 0];
    let corner = [1, 7, 1];
    let edge = [1, 7, 0];
    load(&mut world, center, &[([15, 111, 15], 3)]);
    let missing = world.mesh_snapshot(center).unwrap();
    load(&mut world, corner, &[([16, 112, 16], 3)]);
    let after_corner = world.mesh_snapshot(center).unwrap();
    load(&mut world, edge, &[([16, 112, 15], 3)]);
    let after_edge = world.mesh_snapshot(center).unwrap();
    assert!(world.set([16, 112, 16], 0));

    let face = missing
        .greedy_mesh_local()
        .into_iter()
        .find(|f| f.normal == [0, 1, 0])
        .unwrap();
    assert_eq!(tops(&missing, 16.0), [(face, [3, 3, 3, 3])]);
    assert_eq!(tops(&after_corner, 16.0), [(face, [3, 3, 2, 3])]);
    assert_eq!(tops(&after_edge, 16.0), [(face, [3, 3, 1, 2])]);
    assert_eq!(
        tops(&world.mesh_snapshot(center).unwrap(), 16.0),
        [(face, [3, 3, 2, 2])]
    );
}

#[test]
fn ao_two_side_blockers_make_a_dark_corner() {
    let mut world = World::streamed(99);
    let chunk = [0, 7, 0];
    load(
        &mut world,
        chunk,
        &[([5, 118, 5], 3), ([4, 119, 5], 3), ([5, 119, 4], 3)],
    );
    let snapshot = world.mesh_snapshot(chunk).unwrap();
    let face = snapshot
        .greedy_mesh_local()
        .into_iter()
        .find(|f| f.normal == [0, 1, 0] && f.corners[0] == [5.0, 7.0, 5.0])
        .unwrap();
    assert_eq!(tops(&snapshot, 7.0), [(face, [0, 2, 3, 2])]);
}

#[test]
fn negative_face_keeps_ao_in_outward_corner_order() {
    let mut world = World::streamed(99);
    let chunk = [0, 7, 0];
    load(&mut world, chunk, &[([5, 118, 5], 3), ([4, 117, 5], 3)]);
    let snapshot = world.mesh_snapshot(chunk).unwrap();
    let (face, light) = snapshot
        .greedy_ao_local()
        .into_iter()
        .find(|(face, _)| face.normal == [0, -1, 0] && face.corners[0][1] == 6.0)
        .unwrap();
    assert_eq!(
        face.corners,
        [
            [5.0, 6.0, 5.0],
            [6.0, 6.0, 5.0],
            [6.0, 6.0, 6.0],
            [5.0, 6.0, 6.0],
        ],
    );
    assert_eq!(light, [2, 3, 3, 2]);
}

#[test]
fn flat_same_material_faces_merge_until_their_corner_levels_vary() {
    let mut world = World::streamed(99);
    let chunk = [0, 7, 0];
    load(&mut world, chunk, &[([4, 118, 5], 3), ([5, 118, 5], 3)]);
    let flat = tops(&world.mesh_snapshot(chunk).unwrap(), 7.0);
    assert_eq!(flat.len(), 1);
    assert_eq!(flat[0].0.corners[2], [6.0, 7.0, 6.0]);
    assert_eq!(flat[0].1, [3, 3, 3, 3]);

    assert!(world.set([4, 119, 4], 3));
    let shaded = tops(&world.mesh_snapshot(chunk).unwrap(), 7.0);
    assert_eq!(shaded.len(), 2);
    assert_eq!(shaded[0].0.corners[2], [5.0, 7.0, 6.0]);
    assert_eq!(shaded[0].1, [2, 3, 3, 2]);
    assert_eq!(shaded[1].0.corners[2], [6.0, 7.0, 6.0]);
    assert_eq!(shaded[1].1, [2, 3, 3, 3]);
}
