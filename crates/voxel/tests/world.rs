use primal_voxel::{MAX_BLOCK, World, dirty_chunks};

#[test]
fn generator_v1_seed42_matches_persisted_world_fingerprint() {
    assert_eq!(primal_voxel::GENERATOR_VERSION, 1);
    let world = World::new(42);
    let mut fingerprint = 0xcbf2_9ce4_8422_2325_u64;
    for y in 0..64 {
        for z in -64..64 {
            for x in -64..64 {
                fingerprint ^= u64::from(world.get([x, y, z]));
                fingerprint = fingerprint.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    // FNV-1a over every voxel, ordered Y then Z then X. Preserve v1 saves by
    // introducing a new generator version before intentionally changing terrain.
    assert_eq!(fingerprint, 10_711_883_335_786_081_859);
}

#[test]
fn edits_restore_original_and_reject_invalid_input() {
    let mut world = World::new(42);
    let position = [0, 30, 0];
    let original = world.get(position);
    let replacement = if original == 0 { 3 } else { 0 };
    assert!(world.set(position, replacement));
    assert_eq!(world.get(position), replacement);
    assert_eq!(world.edits(), vec![(position, replacement)]);
    assert!(!world.set(position, replacement));
    assert!(world.set(position, original));
    assert!(world.edits().is_empty());
    assert!(!world.set([64, 5, 0], 1));
    assert!(!world.set([0, -1, 0], 1));
    assert!(!world.set(position, MAX_BLOCK + 1));
}

#[test]
fn generation_and_edit_replay_are_deterministic() {
    let mut first = World::new(773);
    let mut replay = World::new(first.seed());
    for position in [[-64, 2, -64], [-1, 24, 15], [63, 63, 63]] {
        assert_eq!(first.get(position), replay.get(position));
        first.set(position, 7);
    }
    for (position, block) in first.edits() {
        replay.set(position, block);
    }
    assert_eq!(first.edits(), replay.edits());
    for chunk in first.chunks() {
        assert_eq!(first.mesh(chunk), replay.mesh(chunk));
    }
    assert_ne!(World::new(1).mesh([0, 0, 0]), World::new(2).mesh([0, 0, 0]));
}

#[test]
fn adjoining_voxels_cull_faces_across_negative_chunk_boundary() {
    let mut world = World::new(42);
    // The upper sky is empty. Positions straddle chunks -1 and 0.
    assert_eq!(world.get([-1, 60, 0]), 0);
    world.set([-1, 60, 0], 3);
    world.set([0, 60, 0], 3);
    let left = world.mesh([-1, 3, 0]);
    let right = world.mesh([0, 3, 0]);
    assert_eq!(left.len() + right.len(), 10);
    assert!(!left.iter().any(|face| face.normal == [1, 0, 0]));
    assert!(!right.iter().any(|face| face.normal == [-1, 0, 0]));
    world.set([0, 60, 0], 0);
    assert_eq!(world.mesh([-1, 3, 0]).len(), 6);
}

#[test]
fn mesh_winding_matches_outward_normal() {
    let mut world = World::new(42);
    world.set([0, 60, 0], 3);
    for face in world.mesh([0, 3, 0]) {
        let a = std::array::from_fn::<_, 3, _>(|i| face.corners[1][i] - face.corners[0][i]);
        let b = std::array::from_fn::<_, 3, _>(|i| face.corners[2][i] - face.corners[0][i]);
        let cross = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        assert_eq!(cross, face.normal.map(|value| value as f32));
    }
}

#[test]
fn dirty_chunks_only_include_face_neighbours_inside_world() {
    assert_eq!(dirty_chunks([1, 1, 1]), vec![[0, 0, 0]]);
    let dirty = dirty_chunks([0, 16, -1]);
    assert_eq!(dirty.len(), 4);
    for chunk in [[0, 1, -1], [-1, 1, -1], [0, 0, -1], [0, 1, 0]] {
        assert!(dirty.contains(&chunk));
    }
    assert_eq!(dirty_chunks([-64, 0, -64]), vec![[-4, 0, -4]]);
    assert!(dirty_chunks([64, 0, 0]).is_empty());
}

#[test]
fn world_has_finite_boundaries_and_clear_spawn() {
    let world = World::new(42);
    assert_eq!(world.chunks().len(), 256);
    assert!(World::contains([-64, 0, -64]));
    assert!(World::contains([63, 63, 63]));
    assert!(!World::contains([0, 64, 0]));
    assert_eq!(world.get([i32::MIN, 0, 0]), 0);
    let height = (0..64).rev().find(|&y| world.get([0, y, 0]) != 0).unwrap();
    assert!(height < 25);
    assert_eq!(world.get([0, height + 1, 0]), 0);
    assert_eq!(world.get([0, height + 2, 0]), 0);
}
