use primal_voxel::{CHUNK, Preset, World, chunk_at, generate_chunk, generate_for, surface_for};

const SEED: u64 = 20261120;
const FOREST: Preset = Preset::ForestModern;

#[test]
fn historical_preset_keeps_legacy_chunk_bytes() {
    for chunk in [[0, 0, 0], [-4, 1, 2], [16, 3, -12]] {
        let legacy = generate_chunk(SEED, chunk).unwrap();
        let historical = generate_for(Preset::Historical, SEED, chunk).unwrap();
        let origin = chunk.map(|v| v * CHUNK);
        for y in 0..CHUNK {
            for z in 0..CHUNK {
                for x in 0..CHUNK {
                    let position = [origin[0] + x, origin[1] + y, origin[2] + z];
                    assert_eq!(historical.get(position), legacy.get(position));
                }
            }
        }
    }
}

#[test]
fn world_rejects_generated_data_from_another_preset() {
    let mut world = World::with_preset(SEED, FOREST);
    let chunk = [0, 0, 0];
    let ticket = world.begin_load(chunk).unwrap();
    let historical = generate_chunk(SEED, chunk).unwrap();
    assert!(!world.install_chunk(ticket, historical, &[]));
    let ticket = world.begin_load(chunk).unwrap();
    let forest = generate_for(world.preset(), world.seed(), chunk).unwrap();
    assert!(world.install_chunk(ticket, forest, &[]));
}

#[test]
fn clearing_has_walkable_ground_and_open_sky() {
    for x in [-9, -1, 0, 9] {
        for z in [-9, -1, 0, 9] {
            let surface = surface_for(FOREST, SEED, x, z);
            assert!((surface - surface_for(FOREST, SEED, 0, 0)).abs() <= 1);
            let ground = [x, surface, z];
            let chunk = generate_for(FOREST, SEED, chunk_at(ground)).unwrap();
            assert_eq!(chunk.get(ground), Some(1));
            for y in surface + 1..64 {
                let air = [x, y, z];
                let chunk = generate_for(FOREST, SEED, chunk_at(air)).unwrap();
                assert_eq!(chunk.get(air), Some(0), "{air:?} should be free");
            }
        }
    }
}

#[test]
fn forest_chunks_are_deterministic_and_vary_with_seed() {
    let chunk = [3, 2, 2];
    let first = generate_for(FOREST, SEED, chunk).unwrap();
    let duplicate = generate_for(FOREST, SEED, chunk).unwrap();
    let other = generate_for(FOREST, SEED + 1, chunk).unwrap();
    let mut changes = 0;
    for y in 32..48 {
        for z in 32..48 {
            for x in 48..64 {
                let position = [x, y, z];
                assert_eq!(first.get(position), duplicate.get(position));
                changes += usize::from(first.get(position) != other.get(position));
            }
        }
    }
    assert!(changes > 100);
}

#[test]
fn forest_spans_the_valid_horizontal_coordinate_domain() {
    for x in [i32::MIN, i32::MAX] {
        for z in [i32::MIN, i32::MAX] {
            let position = [x, surface_for(FOREST, SEED, x, z), z];
            let chunk = generate_for(FOREST, SEED, chunk_at(position)).unwrap();
            assert!(chunk.get(position).unwrap() > 0);
        }
    }
    assert!(generate_for(FOREST, SEED, [0, -1, 0]).is_none());
    assert!(generate_for(FOREST, SEED, [0, 8, 0]).is_none());
}
