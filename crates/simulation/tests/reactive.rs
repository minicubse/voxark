use primal_simulation::creatures::{Behavior, CreatureWorld, Decision, Locomotion};
use primal_voxel::Preset;

fn reactive() -> CreatureWorld {
    CreatureWorld::with_behavior(20261120, Preset::ForestModern, 2, 60, Behavior::Reactive)
}

#[test]
fn hysteresis_target_choice_and_return_follow_authoritative_ticks() {
    let mut w = reactive();
    let home = w.states(0).next().unwrap();
    let mut p = home.position;
    p[0] += 12.01;
    let mut tick = 0;
    let mut run = |world: &mut CreatureWorld, ticks, players: &[(u64, [f64; 3])]| {
        for _ in 0..ticks {
            tick += 1;
            world.advance(tick, players.iter().copied(), |_| true);
        }
        world.states(tick).next().unwrap()
    };
    assert_eq!(run(&mut w, 120, &[]), home);
    assert_eq!(run(&mut w, 1, &[(1, p)]), home);
    p[0] -= 0.02;
    let mut other = p;
    other[0] = home.position[0] - (p[0] - home.position[0]);
    run(&mut w, 1, &[(8, other), (2, p)]);
    assert_eq!(w.decision(), Some((Decision::Observe, Some(2))));
    let watching = run(&mut w, 80, &[(2, p)]);
    assert_eq!(watching.position, home.position);
    assert_ne!(watching.yaw, home.yaw);
    let walking = run(&mut w, 40, &[(2, p)]);
    assert_eq!(walking.locomotion, Locomotion::Walk);
    assert!(walking.position[0] > home.position[0]);
    let stopped = run(&mut w, 500, &[(2, p)]);
    assert_eq!(w.decision(), Some((Decision::Stop, Some(2))));
    assert!((p[0] - stopped.position[0] - 3.5).abs() < 0.01);
    p[0] += 0.35;
    assert_eq!(
        run(&mut w, 120, &[(2, p)]).state_start_tick,
        stopped.state_start_tick
    );
    p[0] += 1.5;
    assert_eq!(run(&mut w, 5, &[(2, p)]).locomotion, Locomotion::Walk);
    // Inside the 12..16 m hysteresis band the existing target is retained.
    p[0] = stopped.position[0] + 15.;
    run(&mut w, 1, &[(2, p), (8, stopped.position)]);
    assert_eq!(w.decision().unwrap().1, Some(2));
    p[0] += 3.;
    run(&mut w, 1, &[(2, p)]);
    assert_eq!(w.decision(), Some((Decision::Return, None)));
    let back = run(&mut w, 600, &[]);
    assert_eq!(back.position, home.position);
    assert_eq!(back.locomotion, Locomotion::IdleGround);
    assert_eq!(w.decision(), Some((Decision::Rest, None)));
}

#[test]
fn replay_is_deterministic_and_only_first_slot_is_reactive() {
    let mut a = reactive();
    let mut b = reactive();
    let mut demo = CreatureWorld::new(20261120, Preset::ForestModern, 2, 60);
    let explicit =
        CreatureWorld::with_behavior(20261120, Preset::ForestModern, 2, 60, Behavior::Demo);
    let home = a.states(0).next().unwrap().position;
    for tick in 0..2500 {
        let p = [
            home[0] + 8.,
            home[1],
            home[2] + (tick as f64 / 90.).sin() * 2.,
        ];
        let mut players = vec![(9, p), (3, p)];
        if tick >= 500 {
            players.retain(|x| x.0 != 3);
        }
        if tick >= 1000 {
            players.clear();
        }
        a.advance(tick, players.iter().copied(), |_| true);
        b.advance(tick, players.iter().rev().copied(), |_| true);
        demo.advance(tick, players.iter().copied(), |_| false);
        assert!(a.states(tick).eq(b.states(tick)));
        assert!(demo.states(tick).eq(explicit.states(tick)));
        assert_eq!(a.states(tick).nth(1), demo.states(tick).nth(1));
        if tick == 500 {
            assert_eq!(a.decision(), Some((Decision::Return, None)));
        }
    }
}

#[test]
fn blocked_steps_and_distant_or_vertical_players_are_safe() {
    let mut w = reactive();
    let home = w.states(0).next().unwrap();
    let mut p = home.position;
    for t in 0..300 {
        w.advance(
            t,
            [(1, [1e100, 0., -1e100]), (2, [p[0], p[1] + 18., p[2]])].into_iter(),
            |_| panic!("unexpected move"),
        );
    }
    assert_eq!(w.states(299).next().unwrap(), home);
    p[0] += 9.;
    for t in 300..500 {
        w.advance(t, [(1, p)].into_iter(), |_| false);
    }
    let blocked = w.states(499).next().unwrap();
    assert_eq!(blocked.position, home.position);
    assert_eq!(blocked.locomotion, Locomotion::IdleGround);
    for t in 500..2000 {
        let s = w.states(t - 1).next().unwrap();
        p = [s.position[0], s.position[1], s.position[2] + 10.];
        w.advance(t, [(1, p)].into_iter(), |_| true);
        let s = w.states(t).next().unwrap();
        assert!(s.position[2] < 16. && s.position[2] > -16.);
    }
    assert!(w.states(1999).next().unwrap().position[2] > 15.);
    assert!(w.despawn(home.id));
    assert!(w.decision().is_none());
    let new = w.spawn().unwrap();
    w.advance(2000, std::iter::empty(), |_| true);
    assert_ne!(home.id, new);
    assert_eq!(w.sample(new, 2000).unwrap().position, home.position);
}

/// Archived server frames from benchmarks/results/4a/acceptance/same-tick.jsonl.
#[test]
fn demo_matches_archived_authority_across_all_six_clips() {
    let demo = CreatureWorld::new(20261120, Preset::ForestModern, 1, 60);
    let fixtures = [
        (
            1212,
            Locomotion::IdleGround,
            1204,
            [-2.8291746923771393, 25.0, 7.774379414605454],
            1.1415926218032837_f64,
        ),
        (
            360,
            Locomotion::Walk,
            300,
            [-3.350751679170889, 25.0, 7.5122543811075895],
            1.0686089992523193_f64,
        ),
        (
            489,
            Locomotion::Takeoff,
            487,
            [-6.065926633055459, 25.00477990137854, 5.070405611029837],
            0.6080897450447083_f64,
        ),
        (
            552,
            Locomotion::Flap,
            550,
            [-7.36493993296703, 28.13407303909482, 1.9638032145136737],
            0.18401210010051727_f64,
        ),
        (
            828,
            Locomotion::Glide,
            826,
            [12.99907813441094, 44.99876251706873, -5.806587491654289],
            -2.6743004322052_f64,
        ),
        (
            1140,
            Locomotion::Land,
            1138,
            [0.8830368695795618, 27.86652753429613, 8.490824910892659],
            1.6186943054199219_f64,
        ),
    ];
    for (tick, clip, began, position, yaw) in fixtures {
        let state = demo.states(tick).next().unwrap();
        assert_eq!((state.locomotion, state.state_start_tick), (clip, began));
        for (actual, reference) in state.position.into_iter().zip(position) {
            assert!((actual - reference).abs() < 1e-10);
        }
        assert!((f64::from(state.yaw) - yaw).abs() < 1e-6);
    }
}
