//! Local, deterministic, four-neighbour A*. All scratch space belongs to slot zero.
use super::PATCH_EDGE;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

const SIDE: usize = 2 * PATCH_EDGE as usize;
const CELLS: usize = SIDE * SIDE;
const POPS_PER_TICK: usize = 6;
const MAX_POPS: usize = 192;
type Point = [f64; 2];

fn cell([x, z]: Point) -> Option<usize> {
    let limit = f64::from(PATCH_EDGE) - 0.5;
    if x.abs() <= limit && z.abs() <= limit {
        Some(
            (z.floor() as i32 + PATCH_EDGE) as usize * SIDE
                + (x.floor() as i32 + PATCH_EDGE) as usize,
        )
    } else {
        None
    }
}

fn center(i: usize) -> Point {
    [
        (i % SIDE) as f64 - f64::from(PATCH_EDGE) + 0.5,
        (i / SIDE) as f64 - f64::from(PATCH_EDGE) + 0.5,
    ]
}

fn distance(a: usize, b: usize) -> u16 {
    ((a % SIDE).abs_diff(b % SIDE) + (a / SIDE).abs_diff(b / SIDE)) as u16
}

#[derive(Default)]
enum Phase {
    #[default]
    Empty,
    Searching,
    Following,
    Failed,
}

pub(super) struct Navigator {
    phase: Phase,
    target: Option<usize>,
    next_search: u64,
    open: BinaryHeap<Reverse<(u16, u16, usize)>>,
    cost: [u16; CELLS],
    parent: [usize; CELLS],
    walkable: [Option<bool>; CELLS],
    path: Vec<usize>,
    pops: usize,
}

impl Default for Navigator {
    fn default() -> Self {
        Self {
            phase: Phase::Empty,
            target: None,
            next_search: 0,
            open: BinaryHeap::new(),
            cost: [u16::MAX; CELLS],
            parent: [0; CELLS],
            walkable: [None; CELLS],
            path: Vec::new(),
            pops: 0,
        }
    }
}

impl Navigator {
    pub(super) fn invalidate(&mut self) {
        self.phase = Phase::Empty;
        self.path.clear();
    }

    fn begin(&mut self, start: usize, end: usize) {
        self.cost.fill(u16::MAX);
        self.walkable.fill(None);
        self.open.clear();
        self.path.clear();
        self.pops = 0;
        self.cost[start] = 0;
        self.parent[start] = start;
        self.open.push(Reverse((distance(start, end), 0, start)));
        self.phase = Phase::Searching;
    }

    fn search(&mut self, end: usize, clear: impl Fn(Point) -> bool) {
        for _ in 0..POPS_PER_TICK {
            let Some(Reverse((_, cost, node))) = self.open.pop() else {
                self.phase = Phase::Failed;
                break;
            };
            if self.pops == MAX_POPS {
                self.phase = Phase::Failed;
                break;
            }
            self.pops += 1;
            if cost != self.cost[node] {
                continue;
            }
            if node == end {
                let mut cursor = end;
                loop {
                    self.path.push(cursor);
                    if cursor == self.parent[cursor] {
                        break;
                    }
                    cursor = self.parent[cursor];
                }
                self.phase = Phase::Following;
                break;
            }
            let [x, z] = center(node);
            for p in [[x - 1., z], [x + 1., z], [x, z - 1.], [x, z + 1.]] {
                let Some(next) = cell(p) else { continue };
                if cost + 1 >= self.cost[next] {
                    continue;
                }
                if !*self.walkable[next].get_or_insert_with(|| clear(p)) {
                    continue;
                }
                if !clear([(p[0] + x) * 0.5, (p[1] + z) * 0.5]) {
                    continue;
                }
                self.cost[next] = cost + 1;
                self.parent[next] = node;
                self.open
                    .push(Reverse((cost + 1 + distance(next, end), cost + 1, next)));
            }
        }
    }

    /// Returns a steering point and whether stopping distance applies on this segment.
    pub(super) fn steer(
        &mut self,
        tick: u64,
        hz: u32,
        from: Point,
        goal: Point,
        approaching: bool,
        free: impl Fn(Point) -> bool,
    ) -> Option<(Point, bool)> {
        let end = cell(goal);
        if end != self.target {
            self.invalidate();
            self.target = end;
        }
        let end = end?;
        let start = cell(from)?;
        if matches!(self.phase, Phase::Empty | Phase::Failed) && tick >= self.next_search {
            self.next_search = tick.saturating_add(u64::from(hz).div_ceil(2));
            self.begin(start, end);
        }
        if matches!(self.phase, Phase::Searching) {
            self.search(end, &free);
            if matches!(self.phase, Phase::Failed) {
                self.next_search = tick.saturating_add(u64::from(hz));
            }
        }
        if !matches!(self.phase, Phase::Following) {
            return None;
        }
        let close = |a: Point, b: Point| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-7;
        while self.path.last().is_some_and(|&i| close(center(i), from)) {
            self.path.pop();
        }
        if let Some(&next) = self.path.last() {
            // Near the target, stop on the first unobstructed approach. Avoid cutting
            // a corner just because the player is within the stopping sphere.
            if approaching
                && (from[0] - goal[0]).hypot(from[1] - goal[1]) <= super::reactive::STOP + 0.04
                && (1..=16).all(|i| {
                    let t = f64::from(i) / 16.;
                    free([
                        from[0] + (goal[0] - from[0]) * t,
                        from[1] + (goal[1] - from[1]) * t,
                    ])
                })
            {
                return Some((goal, true));
            }
            Some((center(next), false))
        } else {
            Some((goal, true))
        }
    }
}
