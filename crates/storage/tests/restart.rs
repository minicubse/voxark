use primal_storage::{WorldLock, load_or_create, save};
use primal_voxel::{Preset, World};
use std::fs;
use std::io::ErrorKind;

#[test]
fn two_restarts_keep_seed_edits_then_undo() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vallee.pvx");
    let mut world = load_or_create(&path, 23).unwrap();
    let ground = [0, 0, 0];
    let original = world.get(ground);
    assert_ne!(original, 0);
    assert!(world.set(ground, 0));
    assert!(world.set([-16, 63, 15], 3));
    save(&path, &world).unwrap();
    drop(world);
    let mut reopened = load_or_create(&path, 99).unwrap();
    assert_eq!(reopened.seed(), 23);
    assert_eq!(reopened.get(ground), 0);
    assert_eq!(reopened.get([-16, 63, 15]), 3);
    assert!(reopened.set(ground, original));
    save(&path, &reopened).unwrap();
    assert_eq!(
        load_or_create(&path, 0).unwrap().edits(),
        vec![([-16, 63, 15], 3)]
    );
}

#[test]
fn damaged_snapshot_is_rejected_and_left_intact() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vallee.pvx");
    let world = load_or_create(&path, 15).unwrap();
    save(&path, &world).unwrap();
    let valid = fs::read(&path).unwrap();
    let mut flipped = valid.clone();
    flipped[8] ^= 1;
    for invalid in [flipped, valid[..valid.len() - 1].to_vec(), vec![]] {
        fs::write(&path, &invalid).unwrap();
        assert_eq!(
            load_or_create(&path, 15).err().unwrap().kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(fs::read(&path).unwrap(), invalid);
    }
}

#[test]
fn interrupted_staging_file_does_not_replace_last_commit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/world.pvx");
    let first = WorldLock::acquire(&path).unwrap();
    assert!(WorldLock::acquire(&path).is_err());
    let mut world = load_or_create(&path, 84).unwrap();
    world.set([12, 63, 32], 5);
    save(&path, &world).unwrap();
    fs::write(
        path.parent().unwrap().join(".tmp-unfinished"),
        b"incomplete",
    )
    .unwrap();
    drop(first);
    let _next_session = WorldLock::acquire(&path).unwrap();
    assert_eq!(load_or_create(&path, 0).unwrap().get([12, 63, 32]), 5);
}

#[test]
fn valid_checksum_does_not_hide_incompatible_or_invalid_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vallee.pvx");
    let mut world = load_or_create(&path, 42).unwrap();
    world.set([2, 63, 7], 3);
    world.set([3, 63, 7], 5);
    save(&path, &world).unwrap();
    let valid = fs::read(&path).unwrap();
    for (offset, value) in [(4, 2), (6, 2), (16, 40), (20, 64), (23, 98), (24, 2)] {
        let mut bytes = valid.clone();
        bytes[offset] = value;
        let split = bytes.len() - 4;
        let crc = crc32fast::hash(&bytes[..split]).to_le_bytes();
        bytes[split..].copy_from_slice(&crc);
        fs::write(&path, bytes).unwrap();
        assert_eq!(
            load_or_create(&path, 0).err().unwrap().kind(),
            ErrorKind::InvalidData
        );
    }
}

#[test]
fn legacy_snapshot_writer_rejects_forest_without_touching_an_existing_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vallee.pvx");
    let historical = load_or_create(&path, 42).unwrap();
    save(&path, &historical).unwrap();
    let original = fs::read(&path).unwrap();
    let forest = World::with_preset(42, Preset::ForestModern);
    assert_eq!(
        save(&path, &forest).unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
    assert_eq!(fs::read(path).unwrap(), original);
}
