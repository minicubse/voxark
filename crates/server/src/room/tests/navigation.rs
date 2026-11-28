use super::{Behavior, CreatureWorld, Decision, World, forest, ground_step_clear};
use primal_simulation::creatures::Locomotion;
use std::cell::Cell;

struct Course {
    world: World,
    actors: CreatureWorld,
    home: [f64; 3],
    player: [f64; 3],
    tick: u64,
}

impl Course {
    fn new(mode: Behavior) -> Self {
        let world = forest();
        let actors = CreatureWorld::with_behavior(world.seed(), world.preset(), 1, 60, mode);
        let home = actors.states(0).next().unwrap().position;
        let player = [home[0] + 8., home[1] + 1., home[2]];
        Self {
            world,
            actors,
            home,
            player,
            tick: 0,
        }
    }
    fn edit(&mut self, pos: [i32; 3], value: u8) {
        assert!(self.world.set(pos, value));
        self.actors.terrain_edited(pos);
    }
    fn barrier(&mut self, sealed: bool, hole: bool) {
        let range = if sealed { -16..=15 } else { 6..=9 };
        for z in range {
            let x = -1;
            let top = primal_voxel::surface_for(self.world.preset(), self.world.seed(), x, z);
            self.edit(
                [x, if hole { top } else { top + 1 }, z],
                if hole { 0 } else { 3 },
            );
        }
    }
    fn step(&mut self, nearby: bool) -> usize {
        let probes = Cell::new(0);
        self.tick += 1;
        self.actors.advance(
            self.tick,
            nearby.then_some((1, self.player)).into_iter(),
            |p| {
                probes.set(probes.get() + 1);
                ground_step_clear(&self.world, p)
            },
        );
        probes.get()
    }
    fn position(&self) -> [f64; 3] {
        self.actors.states(self.tick).next().unwrap().position
    }
    fn decision(&self) -> Decision {
        self.actors.decision().unwrap().0
    }
}

#[test]
fn goes_around_wall_and_hole_then_returns_on_resident_voxels() {
    for hole in [false, true] {
        let mut c = Course::new(Behavior::Navigation);
        c.barrier(false, hole);
        for approach in [true, false] {
            let mut crossed = false;
            let mut last = c.position();
            for _ in 0..900 {
                assert!(c.step(approach) <= 65);
                let p = c.position();
                assert!(ground_step_clear(&c.world, p));
                assert!((p[0] - last[0]).hypot(p[2] - last[2]) <= 2. / 60. + 1e-8);
                if (-1.35..0.35).contains(&p[0]) {
                    assert!(p[2] <= 5.65 || p[2] >= 10.35);
                    crossed = true;
                }
                last = p;
            }
            assert!(crossed);
            if approach {
                assert_eq!(c.decision(), Decision::Stop);
                let p = c.position();
                assert!(((p[0] - c.player[0]).hypot(p[2] - c.player[2]) - 3.5).abs() < 0.04);
            } else {
                assert_eq!(c.decision(), Decision::Rest);
                assert_eq!(c.position(), c.home);
            }
        }
    }
}

#[test]
fn closed_barrier_stops_and_budgeted_retries_resume_through_opening() {
    let mut c = Course::new(Behavior::Navigation);
    c.barrier(true, false);
    let mut searching = 0;
    for _ in 0..900 {
        let probes = c.step(true);
        assert!(probes <= 65);
        searching += usize::from(probes > 0);
    }
    assert!(searching > 0 && searching < 450);
    assert_eq!(c.position(), c.home);
    assert_eq!(
        c.actors.states(c.tick).next().unwrap().locomotion,
        Locomotion::IdleGround
    );
    c.edit([-1, 25, 7], 0);
    for _ in 0..600 {
        c.step(true);
    }
    assert_eq!(c.decision(), Decision::Stop);
    assert!(c.position()[0] > 0.35);
}

#[test]
fn wall_inserted_while_walking_invalidates_the_current_path() {
    let mut c = Course::new(Behavior::Navigation);
    while c.actors.states(c.tick).next().unwrap().locomotion != Locomotion::Walk {
        c.step(true);
        assert!(c.tick < 200);
    }
    c.barrier(false, false);
    let mut lateral = false;
    for _ in 0..700 {
        c.step(true);
        let p = c.position();
        assert!(ground_step_clear(&c.world, p));
        lateral |= (-1.35..0.35).contains(&p[0]) && !(5.65..10.35).contains(&p[2]);
    }
    assert!(lateral && c.decision() == Decision::Stop);
}

#[test]
fn removing_wall_block_while_walking_replaces_the_still_viable_detour() {
    let mut course = Course::new(Behavior::Navigation);
    course.barrier(false, false);
    while course.actors.states(course.tick).next().unwrap().locomotion != Locomotion::Walk {
        course.step(true);
        assert!(course.tick < 200);
    }
    course.edit([-1, 25, 7], 0);
    let mut crossed_opening = false;
    for _ in 0..600 {
        course.step(true);
        let [x, _, z] = course.position();
        if (-1.0..0.0).contains(&x) {
            assert!((7.35..=7.65).contains(&z));
            crossed_opening = true;
        }
    }
    assert!(crossed_opening);
    assert_eq!(course.decision(), Decision::Stop);
}

#[test]
#[ignore = "release CPU benchmark on the wall, hole and sealed courses"]
fn navigation_cpu() {
    use cpu_time::ThreadTime;
    use std::time::Instant;
    const TICKS: usize = 240_000;
    for (label, sealed, hole) in [
        ("wall", false, false),
        ("hole", false, true),
        ("sealed", true, false),
    ] {
        for mode in [Behavior::Reactive, Behavior::Navigation] {
            let course = || {
                let mut course = Course::new(mode);
                course.barrier(sealed, hole);
                course
            };
            const CPU_TICKS: usize = 10 * TICKS;
            let mut c = course();
            let cpu = ThreadTime::now();
            for tick in 0..CPU_TICKS {
                std::hint::black_box(c.step(tick % 1440 < 720));
            }
            let mean = cpu.elapsed().as_secs_f64() * 1e6 / CPU_TICKS as f64;
            let mut c = course();
            let mut measured = Vec::with_capacity(TICKS);
            let mut peak_probes = 0;
            let mut slow = 0;
            for tick in 0..TICKS {
                let start = Instant::now();
                peak_probes = peak_probes.max(c.step(tick % 1440 < 720));
                let ns = start.elapsed().as_nanos();
                slow += usize::from(ns > 1_000_000);
                measured.push(ns);
            }
            measured.sort_unstable();
            println!(
                "NAV_CPU {label} {mode:?} cpu_ticks={CPU_TICKS} timed_ticks={TICKS} thread_us={mean:.4} p99_us={:.3} p999_us={:.3} max_us={:.3} over_1ms={slow} peak_probes={peak_probes}",
                measured[TICKS * 99 / 100] as f64 / 1000.,
                measured[TICKS * 999 / 1000] as f64 / 1000.,
                measured[TICKS - 1] as f64 / 1000.
            );
        }
    }
}
