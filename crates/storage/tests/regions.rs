use primal_storage::RegionStore;
use primal_voxel::{CHUNK, Preset};
use std::fs;
use std::io::ErrorKind;
use std::sync::{Arc, Barrier};
use std::thread;

// Static v1 fixture, seed 915, three edits in different chunks of the old valley.
const LEGACY: &[u8] = &[
    80, 86, 88, 68, 1, 0, 1, 0, 147, 3, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 192, 63, 192, 7, 0, 63, 0, 2,
    63, 63, 63, 3, 7, 137, 102, 129,
];

#[test]
fn migration_preserves_source_seed_and_latest_regional_edits() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vallee.pvx");
    fs::write(&path, LEGACY).unwrap();
    let store = RegionStore::open(&path, 132).unwrap();
    assert_eq!(store.seed(), 915);
    for (pos, block) in [([-64, 63, -64], 7), ([0, 63, 0], 2), ([63, 63, 63], 3)] {
        let chunk = pos.map(|v: i32| v.div_euclid(CHUNK));
        assert_eq!(store.load_chunk(chunk).unwrap(), [(pos, block)]);
    }
    store.save_chunk([0, 3, 0], &[]).unwrap();
    drop(store);
    let store = RegionStore::open(&path, 19).unwrap();
    assert_eq!(store.seed(), 915);
    assert!(store.load_chunk([0, 3, 0]).unwrap().is_empty());
    assert_eq!(fs::read(&path).unwrap(), LEGACY);
}

#[test]
fn a_legacy_world_cannot_be_opened_as_forest() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vallee.pvx");
    fs::write(&path, LEGACY).unwrap();
    assert_eq!(
        RegionStore::open_with_preset(&path, 26, Some(Preset::ForestModern))
            .err()
            .unwrap()
            .kind(),
        ErrorKind::InvalidInput
    );
    assert!(!RegionStore::directory(&path).exists());
    assert_eq!(fs::read(&path).unwrap(), LEGACY);
    let store = RegionStore::open(&path, 26).unwrap();
    assert_eq!(store.preset(), Preset::Historical);
    assert_eq!(store.load_chunk([0, 3, 0]).unwrap(), [([0, 63, 0], 2)]);
}

#[test]
fn forest_manifest_preserves_seed_and_preset_and_refuses_unknown_preset() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("forest.pvx");
    let store = RegionStore::open_with_preset(&path, 26, Some(Preset::ForestModern)).unwrap();
    let edits = [([-7, 50, 10], 5)];
    store.save_chunk([-1, 3, 0], &edits).unwrap();
    drop(store);
    let store = RegionStore::open(&path, 99).unwrap();
    assert_eq!(store.preset(), Preset::ForestModern);
    assert_eq!(store.seed(), 26);
    assert_eq!(store.load_chunk([-1, 3, 0]).unwrap(), edits);
    drop(store);
    let manifest = RegionStore::directory(&path).join("world.pvxd");
    let mut bytes = fs::read(&manifest).unwrap();
    bytes[16] = 255;
    replace_crc(&mut bytes);
    fs::write(&manifest, &bytes).unwrap();
    assert_eq!(
        RegionStore::open(&path, 99).err().unwrap().kind(),
        ErrorKind::InvalidData
    );
    assert_eq!(fs::read(manifest).unwrap(), bytes);
}

#[test]
fn corrupt_legacy_and_missing_manifest_are_never_treated_as_new_worlds() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vallee.pvx");
    let directory = RegionStore::directory(&path);
    let mut damaged = LEGACY.to_vec();
    damaged[21] ^= 1;
    fs::write(&path, &damaged).unwrap();
    assert_eq!(
        RegionStore::open(&path, 5).err().unwrap().kind(),
        ErrorKind::InvalidData
    );
    assert_eq!(fs::read(&path).unwrap(), damaged);
    assert!(!directory.exists());
    fs::write(&path, LEGACY).unwrap();
    drop(RegionStore::open(&path, 6).unwrap());
    fs::remove_file(directory.join("world.pvxd")).unwrap();
    assert!(RegionStore::open(&path, 7).is_err());
    assert_eq!(fs::read(&path).unwrap(), LEGACY);
    assert!(!directory.join("world.pvxd").exists());
}

#[test]
fn distant_signed_coordinates_roundtrip_and_same_region_chunks_survive_an_undo() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("regions.pvx");
    let store = RegionStore::open(&path, 4096).unwrap();
    let positions = [
        [i32::MIN, 90, i32::MAX],
        [-1_000_001, 65, 1_999_999],
        [-17, 127, -1],
        [-16, 127, -1],
        [0, 64, 0],
        [i32::MAX, 127, i32::MIN],
    ];
    for pos in positions {
        let chunk = pos.map(|v| v.div_euclid(CHUNK));
        store.save_chunk(chunk, &[(pos, 6)]).unwrap();
    }
    drop(store);
    let store = RegionStore::open(&path, 2).unwrap();
    for pos in positions {
        let chunk = pos.map(|v| v.div_euclid(CHUNK));
        assert_eq!(store.load_chunk(chunk).unwrap(), [(pos, 6)]);
    }
    store.save_chunk([-2, 7, -1], &[]).unwrap();
    assert!(store.load_chunk([-2, 7, -1]).unwrap().is_empty());
    assert_eq!(
        store.load_chunk([-1, 7, -1]).unwrap(),
        [([-16, 127, -1], 6)]
    );
    assert_eq!(
        store.load_chunk([i32::MAX, 0, 0]).unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
}

#[test]
fn rejected_snapshot_does_not_damage_siblings_or_overwrite_own_previous_commit() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vallee.pvx");
    let store = RegionStore::open(&path, 21).unwrap();
    let edits = [([15, 70, 15], 7), ([1, 64, 0], 3)];
    let chunk = [0, 4, 0];
    store.save_chunk(chunk, &edits).unwrap();
    for bad in [
        vec![([16, 70, 15], 6)],
        vec![([0, 64, 0], 8)],
        vec![([0, 64, 0], 7), ([0, 64, 0], 2)],
    ] {
        assert_eq!(
            store.save_chunk(chunk, &bad).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
    }
    assert_eq!(store.load_chunk(chunk).unwrap(), [edits[1], edits[0]]);
    let directory = RegionStore::directory(&path);
    let file = directory.join("r.0.1.0.pvr");
    let original = fs::read(&file).unwrap();
    let mut broken = original.clone();
    broken[16] ^= 1;
    // Valid CRC with a region address that does not correspond to the filename.
    let split = broken.len() - 4;
    let crc = crc32fast::hash(&broken[..split]).to_le_bytes();
    broken[split..].copy_from_slice(&crc);
    fs::write(&file, &broken).unwrap();
    assert_eq!(
        store.load_chunk(chunk).unwrap_err().kind(),
        ErrorKind::InvalidData
    );
    assert_eq!(
        store.save_chunk(chunk, &[]).unwrap_err().kind(),
        ErrorKind::InvalidData
    );
    assert_eq!(fs::read(&file).unwrap(), broken);
    store.save_chunk([4, 4, 0], &[([64, 65, 0], 5)]).unwrap();
    assert_eq!(store.load_chunk([4, 4, 0]).unwrap(), [([64, 65, 0], 5)]);
}

#[test]
fn concurrent_chunk_saves_merge_and_session_lock_covers_clones() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vallee.pvx");
    let store = RegionStore::open(&path, 915).unwrap();
    let gate = Arc::new(Barrier::new(4));
    thread::scope(|scope| {
        for cx in 0..4 {
            let shared = store.clone();
            let gate = Arc::clone(&gate);
            scope.spawn(move || {
                gate.wait();
                for block in [3, 7, 4, 2] {
                    shared
                        .save_chunk([cx, 4, 0], &[([cx * CHUNK, 65, 0], block)])
                        .unwrap();
                }
            });
        }
    });
    let remaining = store.clone();
    drop(store);
    assert_eq!(
        RegionStore::open(&path, 2).err().unwrap().kind(),
        ErrorKind::WouldBlock
    );
    drop(remaining);
    let reopened = RegionStore::open(&path, 301).unwrap();
    for cx in 0..4 {
        assert_eq!(
            reopened.load_chunk([cx, 4, 0]).unwrap(),
            [([cx * CHUNK, 65, 0], 2)]
        );
    }
}

fn replace_crc(data: &mut [u8]) {
    let split = data.len() - 4;
    let crc = crc32fast::hash(&data[..split]).to_le_bytes();
    data[split..].copy_from_slice(&crc);
}

#[test]
fn oversized_truncated_and_semantically_invalid_region_data_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vallee.pvx");
    let store = RegionStore::open(&path, 21).unwrap();
    let chunk = [0, 4, 0];
    let edits = [([0, 64, 0], 3), ([0, 64, 1], 3)];
    store.save_chunk(chunk, &edits).unwrap();
    let file = RegionStore::directory(&path).join("r.0.1.0.pvr");
    let valid = fs::read(&file).unwrap();
    let mut damaged = vec![valid[..valid.len() - 1].to_vec(), vec![]];
    let mut bad_crc = valid.clone();
    bad_crc[8] ^= 1;
    damaged.push(bad_crc);
    // Recompute CRC after introducing an invalid format, generator, seed, count or record.
    for (offset, value) in [(4, 3), (6, 3), (8, 9), (28, 8), (32, 8), (37, 0), (39, 4)] {
        let mut record = valid.clone();
        record[offset] = value;
        replace_crc(&mut record);
        damaged.push(record);
    }
    for data in damaged {
        fs::write(&file, &data).unwrap();
        assert_eq!(
            store.load_chunk(chunk).unwrap_err().kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(
            store.save_chunk(chunk, &edits).unwrap_err().kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(fs::read(&file).unwrap(), data);
    }
    // A one-byte-over file is rejected from its metadata before record allocation.
    fs::File::create(&file).unwrap().set_len(1_048_613).unwrap();
    assert_eq!(
        store.load_chunk(chunk).unwrap_err().kind(),
        ErrorKind::InvalidData
    );
}

#[test]
fn invalid_published_manifest_is_preserved_and_prevents_fallback_to_legacy() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vallee.pvx");
    fs::write(&path, LEGACY).unwrap();
    drop(RegionStore::open(&path, 1).unwrap());
    let manifest = RegionStore::directory(&path).join("world.pvxd");
    let valid = fs::read(&manifest).unwrap();
    let mut bad_crc = valid.clone();
    bad_crc[8] ^= 1;
    let mut bad_version = valid.clone();
    bad_version[4] = 19;
    replace_crc(&mut bad_version);
    for data in [bad_crc, bad_version, valid[..18].to_vec()] {
        fs::write(&manifest, &data).unwrap();
        assert_eq!(
            RegionStore::open(&path, 1).err().unwrap().kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(fs::read(&manifest).unwrap(), data);
        assert_eq!(fs::read(&path).unwrap(), LEGACY);
    }
}
