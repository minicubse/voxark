//! The single ground observer owned by route slot zero.
use super::{CreatureState, GroundPatch, Locomotion, PATCH_EDGE};
use serde::Serialize;
use std::f64::consts::{PI, TAU};

const DETECT: f64 = 12.;
const LOSE: f64 = 16.;
const STOP: f64 = 3.5;
const RESUME: f64 = 4.5;
const SPEED: f64 = 2.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum Decision {
    Rest,
    Observe,
    Approach,
    Stop,
    Return,
}

pub(super) struct Reactive {
    home: [f64; 3],
    pub state: CreatureState,
    pub decision: Decision,
    pub target: Option<u64>,
    changed: u64,
}

impl Reactive {
    pub fn new(home: CreatureState) -> Self {
        Self {
            home: home.position,
            state: home,
            decision: Decision::Rest,
            target: None,
            changed: 0,
        }
    }

    fn enter(&mut self, state: Decision, tick: u64) {
        if self.decision != state {
            self.decision = state;
            self.changed = tick;
        }
    }

    fn select(&self, players: impl Iterator<Item = (u64, [f64; 3])>) -> Option<(u64, [f64; 3])> {
        let mut nearest = None;
        let [x, y, z] = self.state.position;
        for (id, pos) in players {
            let d = (pos[0] - x).hypot(pos[2] - z).hypot(pos[1] - y);
            if self.target == Some(id) && d <= LOSE {
                return Some((id, pos));
            }
            if self.target.is_none()
                && d <= DETECT
                && nearest.is_none_or(|(best, id0, _)| (d, id) < (best, id0))
            {
                nearest = Some((d, id, pos));
            }
        }
        nearest.map(|(_, id, pos)| (id, pos))
    }

    pub fn step(
        &mut self,
        tick: u64,
        hz: u32,
        ground: &GroundPatch,
        players: impl Iterator<Item = (u64, [f64; 3])>,
        clear: impl Fn([f64; 3]) -> bool,
    ) {
        let target = self.select(players);
        let hold = (3 * u64::from(hz)).div_ceil(2);
        let elapsed = tick.saturating_sub(self.changed);
        if self.target.is_some() && target.is_none() {
            self.target = None;
            self.enter(Decision::Return, tick);
        } else if self.target.is_none()
            && let Some((id, _)) = target
            && (self.decision == Decision::Rest || elapsed >= hold)
        {
            self.target = Some(id);
            self.enter(Decision::Observe, tick);
        }

        let goal = if self.target.is_some() {
            target.map(|(_, p)| p).unwrap_or(self.home)
        } else {
            self.home
        };
        let [x, _, z] = self.state.position;
        let dx = goal[0] - x;
        let dz = goal[2] - z;
        let dist = dx.hypot(dz);
        if self.decision == Decision::Observe && tick.saturating_sub(self.changed) >= hold {
            self.enter(
                if dist <= STOP {
                    Decision::Stop
                } else {
                    Decision::Approach
                },
                tick,
            );
        } else if self.decision == Decision::Stop && dist >= RESUME && elapsed >= hold {
            self.enter(Decision::Approach, tick);
        }
        let mut moving = false;
        if dist > 1e-8 && self.decision != Decision::Rest {
            let desired = (-dx).atan2(-dz);
            let yaw = f64::from(self.state.yaw);
            let error = (desired - yaw + PI).rem_euclid(TAU) - PI;
            let max_turn = PI / f64::from(hz);
            self.state.yaw =
                ((yaw + error.clamp(-max_turn, max_turn) + PI).rem_euclid(TAU) - PI) as f32;
            let margin = if self.decision == Decision::Return {
                0.
            } else {
                STOP
            };
            let remaining = (dist - margin).max(0.);
            if matches!(self.decision, Decision::Approach | Decision::Return)
                && remaining > 1e-8
                && error.abs() <= PI / 4.
            {
                let step = remaining.min(SPEED / f64::from(hz));
                let (nx, nz) = if step >= dist {
                    (goal[0], goal[2])
                } else {
                    (x + dx / dist * step, z + dz / dist * step)
                };
                // Leave space for the footprint and for interpolation's adjacent corner.
                let limit = f64::from(PATCH_EDGE) - 0.5;
                if nx.abs() <= limit && nz.abs() <= limit {
                    let next = [nx, ground.at(nx, nz), nz];
                    if clear(next) {
                        self.state.position = next;
                        moving = true;
                    }
                }
            }
        }
        let left = (goal[0] - self.state.position[0]).hypot(goal[2] - self.state.position[2]);
        if self.decision == Decision::Approach && left <= STOP + 1e-8 {
            self.enter(Decision::Stop, tick);
        } else if self.decision == Decision::Return && left <= 1e-8 {
            self.enter(Decision::Rest, tick);
        }
        let clip = if moving {
            Locomotion::Walk
        } else {
            Locomotion::IdleGround
        };
        if clip != self.state.locomotion {
            self.state.locomotion = clip;
            self.state.state_start_tick = tick;
        }
    }
}
