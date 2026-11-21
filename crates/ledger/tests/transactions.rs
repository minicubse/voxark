use primal_ledger::{
    ChunkUpdate, InventoryUpdate, Ledger, OperationId, PlayerState, PlayerUpdate, Transaction,
};
use primal_storage::RegionStore;

fn state() -> PlayerState {
    PlayerState {
        id: 1,
        position: [2.5, 63.0, -3.5],
        yaw: 1.25,
        pitch: -0.75,
        flying: true,
    }
}

fn transaction() -> Transaction {
    Transaction {
        id: OperationId([17; 32]),
        chunks: vec![
            ChunkUpdate {
                chunk: [0, 3, 0],
                expected_revision: 0,
                edits: vec![([0, 63, 0], 5)],
            },
            ChunkUpdate {
                chunk: [-5, 3, 0],
                expected_revision: 0,
                edits: vec![([-65, 63, 0], 7)],
            },
        ],
        players: vec![PlayerUpdate {
            token: [42; 32],
            state: state(),
        }],
        inventories: vec![InventoryUpdate {
            token: [42; 32],
            data: vec![1, 2, 3],
        }],
    }
}

#[test]
fn commits_atomic_state_and_survives_restart_with_idempotency() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.pvx");
    {
        let regions = RegionStore::open(&path, 7049).unwrap();
        regions.save_chunk([1, 3, 0], &[([16, 63, 0], 3)]).unwrap();
        let mut ledger = Ledger::open(&path, regions).unwrap();
        let receipt = ledger.commit(&transaction()).unwrap();
        assert!(!receipt.duplicate);
        assert_eq!(receipt.revisions, vec![([0, 3, 0], 1), ([-5, 3, 0], 1)]);
    }
    let regions = RegionStore::open(&path, 1).unwrap();
    let mut ledger = Ledger::open(&path, regions).unwrap();
    assert_eq!(ledger.seed(), 7049);
    assert_eq!(
        ledger.load_chunk([1, 3, 0]).unwrap().edits,
        vec![([16, 63, 0], 3)]
    );
    assert_eq!(ledger.load_chunk([0, 3, 0]).unwrap().revision, 1);
    assert_eq!(ledger.player(&[42; 32]).unwrap(), Some(state()));
    assert_eq!(ledger.inventory(&[42; 32]).unwrap(), Some(vec![1, 2, 3]));
    assert!(ledger.commit(&transaction()).unwrap().duplicate);
    assert_eq!(ledger.load_chunk([0, 3, 0]).unwrap().revision, 1);
    let mut changed = transaction();
    changed.players[0].state.yaw = 3.0;
    assert!(ledger.commit(&changed).is_err());
}

#[test]
fn failed_validation_rolls_back_every_part() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.pvx");
    let mut ledger = Ledger::open(&path, RegionStore::open(&path, 1).unwrap()).unwrap();
    let mut bad = transaction();
    bad.chunks[1].expected_revision = 1;
    assert!(ledger.commit(&bad).is_err());
    assert_eq!(ledger.player(&[42; 32]).unwrap(), None);
    assert_eq!(ledger.inventory(&[42; 32]).unwrap(), None);
    assert_eq!(ledger.load_chunk([0, 3, 0]).unwrap().revision, 0);
    assert!(ledger.load_chunk([0, 3, 0]).unwrap().edits.is_empty());
    assert!(!ledger.commit(&transaction()).unwrap().duplicate);
}

#[test]
fn player_creation_and_existing_v1_edits_remain_stable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.pvx");
    let mut world = primal_voxel::World::new(7049);
    world.set([0, 63, 0], 3);
    primal_storage::save(&path, &world).unwrap();
    let original = std::fs::read(&path).unwrap();
    let player;
    {
        let mut ledger = Ledger::open(&path, RegionStore::open(&path, 1).unwrap()).unwrap();
        assert_eq!(
            ledger.load_chunk([0, 3, 0]).unwrap().edits,
            vec![([0, 63, 0], 3)]
        );
        player = ledger.get_or_create([42; 32], state()).unwrap();
        assert_eq!(player.id, 1);
    }
    let mut ledger = Ledger::open(&path, RegionStore::open(&path, 1).unwrap()).unwrap();
    let mut alternate = state();
    alternate.yaw = 2.0;
    assert_eq!(ledger.get_or_create([42; 32], alternate).unwrap(), player);
    assert_eq!(ledger.get_or_create([43; 32], state()).unwrap().id, 2);
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn clearing_last_deltas_keeps_revision_and_old_retry_cannot_restore_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.pvx");
    let mut ledger = Ledger::open(&path, RegionStore::open(&path, 1).unwrap()).unwrap();
    ledger.commit(&transaction()).unwrap();
    let clear = Transaction {
        id: OperationId([18; 32]),
        chunks: transaction()
            .chunks
            .into_iter()
            .map(|mut update| {
                update.expected_revision = 1;
                update.edits.clear();
                update
            })
            .collect(),
        players: vec![],
        inventories: vec![],
    };
    ledger.commit(&clear).unwrap();
    assert!(ledger.commit(&transaction()).unwrap().duplicate);
    drop(ledger);
    let ledger = Ledger::open(&path, RegionStore::open(&path, 1).unwrap()).unwrap();
    for update in clear.chunks {
        let snapshot = ledger.load_chunk(update.chunk).unwrap();
        assert_eq!(snapshot.revision, 2);
        assert!(snapshot.edits.is_empty());
    }
    assert_eq!(ledger.revisions().unwrap().len(), 2);
}

#[test]
fn invalid_values_duplicates_and_identity_changes_are_rejected_without_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.pvx");
    let mut ledger = Ledger::open(&path, RegionStore::open(&path, 1).unwrap()).unwrap();
    let mut variants = vec![];
    let mut bad = transaction();
    bad.players[0].state.position[0] = f32::NAN;
    variants.push(bad);
    let mut bad = transaction();
    bad.players[0].state.pitch = f32::INFINITY;
    variants.push(bad);
    let mut bad = transaction();
    bad.players[0].state.id = 0;
    variants.push(bad);
    let mut bad = transaction();
    bad.chunks[0].expected_revision = u64::MAX;
    variants.push(bad);
    let mut bad = transaction();
    bad.chunks[0].chunk[0] = i32::MAX;
    variants.push(bad);
    let mut bad = transaction();
    let duplicate = bad.chunks[0].edits[0];
    bad.chunks[0].edits.push(duplicate);
    variants.push(bad);
    let mut bad = transaction();
    bad.chunks.push(bad.chunks[0].clone());
    variants.push(bad);
    let mut bad = transaction();
    bad.players.push(bad.players[0].clone());
    variants.push(bad);
    let mut bad = transaction();
    bad.players.clear();
    variants.push(bad);
    for bad in variants {
        assert!(ledger.commit(&bad).is_err());
        assert!(ledger.players().unwrap().is_empty());
        assert!(ledger.revisions().unwrap().is_empty());
    }
    ledger.commit(&transaction()).unwrap();
    let mut bad = Transaction {
        id: OperationId([19; 32]),
        chunks: vec![],
        players: transaction().players,
        inventories: vec![],
    };
    bad.players[0].state.id = 2;
    assert!(ledger.commit(&bad).is_err());
    bad.players[0].token = [43; 32];
    bad.players[0].state.id = 1;
    assert!(ledger.commit(&bad).is_err());
    assert_eq!(ledger.players().unwrap(), vec![([42; 32], state())]);
}
