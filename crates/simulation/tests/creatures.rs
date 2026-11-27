use primal_simulation::creatures::{CreatureId, CreatureWorld, Locomotion, Species};
use primal_voxel::{Preset, chunk_at, generate_for, surface_for};
use std::collections::{HashMap, HashSet};

const SEED: u64 = 20261120;

#[test]
fn configured_slots_spawn_once_and_can_be_disabled() {
    let empty = CreatureWorld::new(SEED, Preset::ForestModern, 0, 60);
    assert_eq!(empty.states(10).count(), 0);

    let crowded = CreatureWorld::new(SEED, Preset::ForestModern, 99, 60);
    let states: Vec<_> = crowded.states(10).collect();
    assert_eq!(states.len(), 12);
    let ids: HashSet<_> = states.iter().map(|state| state.id).collect();
    assert_eq!(ids.len(), 12);
    for state in &states {
        assert_ne!(state.id.0, 1);
        assert_eq!(state.species, Species::Wyvern);
        assert_eq!(crowded.sample(state.id, 10), Some(*state));
        assert_eq!(empty.sample(state.id, 10), None);
    }
    assert_eq!(crowded.sample(CreatureId(1), 10), None);

    let mut capped = crowded;
    assert!(capped.spawn().is_none());
    let first = states[0].id;
    assert!(capped.despawn(first));
    assert!(!capped.despawn(first));
    assert_eq!(capped.sample(first, 60), None);
    let fresh = capped.spawn().unwrap();
    assert!(!ids.contains(&fresh));
    assert_eq!(capped.states(60).len(), 12);
    let single = CreatureWorld::new(SEED, Preset::ForestModern, 1, 60);
    assert_eq!(single.states(10).next(), states.first().copied());
}

#[test]
fn transitions_keep_the_authoritative_start_tick_at_60_and_30_hz() {
    use Locomotion::{Flap, Glide, IdleGround, Land, Takeoff, Walk};
    for (tick_hz, boundaries) in [
        (60, [0, 300, 487, 550, 826, 1138, 1204]),
        (30, [0, 150, 244, 276, 414, 570, 603]),
    ] {
        let world = CreatureWorld::new(SEED, Preset::ForestModern, 1, tick_hz);
        let cycle = boundaries[6];
        for iteration in [0, 1, 10_000] {
            for (clip, window) in [IdleGround, Walk, Takeoff, Flap, Glide, Land]
                .into_iter()
                .zip(boundaries.windows(2))
            {
                let [start, end] = [window[0], window[1]].map(|tick| tick + iteration * cycle);
                for tick in [start, (start + end) / 2, end - 1] {
                    let state = world.states(tick).next().unwrap();
                    assert_eq!(state.locomotion, clip, "tick {tick}");
                    assert_eq!(state.state_start_tick, start, "tick {tick}");
                }
            }
        }
        let state = world.states(u64::MAX).next().unwrap();
        assert!(u64::MAX - state.state_start_tick < cycle);
    }
}

#[test]
fn trajectory_has_no_jumps_and_faces_the_motion_through_two_cycles() {
    let world = CreatureWorld::new(SEED, Preset::ForestModern, 1, 60);
    let start = world.states(0).next().unwrap();
    let mut previous = start;
    for state in (1..=2408).map(|tick| world.states(tick).next().unwrap()) {
        let delta: [f64; 3] = std::array::from_fn(|i| state.position[i] - previous.position[i]);
        let distance = delta.iter().map(|value| value * value).sum::<f64>().sqrt();
        assert!(distance < 0.32, "movement jump {distance}: {state:?}");
        let yaw = f64::from(state.yaw - previous.yaw);
        assert!(yaw.sin().atan2(yaw.cos()).abs() < 0.10, "yaw jump {yaw}");
        let [dx, _, dz] = delta;
        let (sin, cos) = f64::from(state.yaw).sin_cos();
        let length = dx.hypot(dz);
        if length > 0.001 {
            assert!((-sin * dx - cos * dz) / length > 0.98);
        }
        previous = state;
    }
    assert_eq!(previous.position, start.position);
    assert_eq!(previous.yaw, start.yaw);
}

#[test]
fn generated_demo_route_keeps_floor_clear_and_grounded_pose_near_its_support() {
    for preset in [Preset::ForestModern, Preset::Historical] {
        let world = CreatureWorld::new(SEED, preset, 12, 60);
        let mut generated = HashMap::new();
        for tick in (0..1204).step_by(6) {
            for state in world.states(tick) {
                let cell = state.position.map(|coordinate| coordinate.floor() as i32);
                let [x, _, z] = cell;
                let floor = f64::from(surface_for(preset, SEED, x, z) + 1);
                let clearance = state.position[1] - floor;
                assert!(clearance >= -1e-9, "{preset} at {tick}: {state:?}");
                if matches!(state.locomotion, Locomotion::IdleGround | Locomotion::Walk) {
                    assert!(clearance <= 1.0, "raised grounded pose: {state:?}");
                }
                let chunk = chunk_at(cell);
                assert_eq!(
                    generated
                        .entry(chunk)
                        .or_insert_with(|| generate_for(preset, SEED, chunk).unwrap())
                        .get(cell),
                    Some(0),
                    "{preset} at {tick}: {state:?}"
                );
            }
        }
    }
}

#[test]
fn restarting_rebuilds_identity_with_a_fresh_cycle() {
    let running = CreatureWorld::new(SEED, Preset::ForestModern, 4, 60);
    let restarted = CreatureWorld::new(SEED, Preset::ForestModern, 4, 60);
    for (older, new) in running.states(600).zip(restarted.states(0)) {
        assert_eq!(older.id, new.id);
        assert_eq!(older.palette, new.palette);
        assert_eq!(older.locomotion, Locomotion::Flap);
        assert_eq!(new.locomotion, Locomotion::IdleGround);
        assert_eq!(new.state_start_tick, 0);
        assert_eq!(running.sample(new.id, 0), Some(new));
    }
}

#[test]
fn interest_is_horizontal_and_remains_precise_far_from_origin() {
    let world = CreatureWorld::new(SEED, Preset::ForestModern, 1, 60);
    let mut state = world.states(0).next().unwrap();
    assert!(state.within_horizontal([0.5, 16.0, 0.5], 16.0));
    state.position = [1_000_000_000.125, 20.0, -1_000_000_000.0];
    for (offset, expected) in [(3.75, true), (4.0, true), (4.125, false)] {
        assert_eq!(
            state.within_horizontal([state.position[0] + offset, 100.0, state.position[2]], 4.0),
            expected
        );
    }
    assert!(!state.within_horizontal([f64::NAN, 0.0, 0.0], 10.0));
    assert!(!state.within_horizontal(state.position, -1.0));
}
