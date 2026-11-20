use std::str::FromStr;

use primal_simulation::{Edit, Player, edit};
use primal_voxel::{CHUNK, Pos, World};

pub const WARMUP: usize = 120;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Scenario {
    #[default]
    Valley,
    Region512,
    Region1024,
    Traverse,
    Teleport,
    Edits,
    Walk,
    Sprint,
    Fly,
    Return,
    MovingEdits,
    LongFlight,
}

impl FromStr for Scenario {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, String> {
        match value {
            "valley" => Ok(Self::Valley),
            "region512" => Ok(Self::Region512),
            "region1024" => Ok(Self::Region1024),
            "traverse" => Ok(Self::Traverse),
            "teleport" => Ok(Self::Teleport),
            "walk" => Ok(Self::Walk),
            "sprint" => Ok(Self::Sprint),
            "fly" => Ok(Self::Fly),
            "return" => Ok(Self::Return),
            "moving-edits" => Ok(Self::MovingEdits),
            "long-flight" => Ok(Self::LongFlight),
            "edits" => Ok(Self::Edits),
            _ => Err(
                "--scenario attend valley, region512, region1024, traverse, teleport ou edits"
                    .into(),
            ),
        }
    }
}

pub struct Flight {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub yaw: f32,
}

impl Scenario {
    pub fn name(self) -> &'static str {
        match self {
            Self::Valley => "valley",
            Self::Region512 => "region512",
            Self::Region1024 => "region1024",
            Self::Traverse => "traverse",
            Self::Teleport => "teleport",
            Self::Edits => "edits",
            Self::Walk => "walk",
            Self::Sprint => "sprint",
            Self::Fly => "fly",
            Self::Return => "return",
            Self::MovingEdits => "moving-edits",
            Self::LongFlight => "long-flight",
        }
    }

    fn route(self, t: f32) -> Option<[f32; 2]> {
        let lerp = |a: [f32; 2], b: [f32; 2], u: f32| {
            std::array::from_fn(|i| a[i] + (b[i] - a[i]) * u.clamp(0.0, 1.0))
        };
        match self {
            Self::Valley
            | Self::Edits
            | Self::Walk
            | Self::Sprint
            | Self::Fly
            | Self::MovingEdits => None,
            Self::Return => Some(lerp(
                [0.5; 2],
                [256.5, 64.5],
                if t < 0.45 {
                    t / 0.45
                } else {
                    1. - (t - 0.45) / 0.45
                },
            )),
            Self::LongFlight => Some(lerp([0.5; 2], [8192.5, -2047.5], t / 0.90)),
            Self::Region512 | Self::Region1024 => {
                let half = if self == Self::Region512 {
                    256.0
                } else {
                    512.0
                };
                let first = [-half + 0.5; 2];
                let last = [half - 0.5; 2];
                Some(if t < 0.25 {
                    lerp([0.5; 2], first, t / 0.25)
                } else {
                    lerp(first, last, (t - 0.25) / 0.55)
                })
            }
            Self::Traverse => Some(lerp([0.5; 2], [4096.5, -1023.5], t / 0.80)),
            Self::Teleport => Some(if (0.20..0.70).contains(&t) {
                [65_536.5, -32_767.5]
            } else {
                [0.5; 2]
            }),
        }
    }

    pub fn flight(self, frame: usize, frames: usize) -> Option<Flight> {
        let elapsed = frame.checked_sub(WARMUP)?;
        let duration = frames.saturating_sub(WARMUP + 1).max(1) as f32;
        let [x, z] = self.route(elapsed as f32 / duration)?;
        let previous = self.route(elapsed.saturating_sub(1) as f32 / duration)?;
        let dx = x - previous[0];
        let dz = z - previous[1];
        let moving = dx != 0.0 || dz != 0.0;
        // A teleport has no continuous trajectory between its two endpoints.
        let velocity = if self == Self::Teleport {
            [0.0; 3]
        } else {
            [dx * 60.0, 0.0, dz * 60.0]
        };
        let yaw = if moving && self != Self::Teleport {
            dx.atan2(dz)
        } else {
            0.78
        };
        Some(Flight {
            position: [x, 52.0, z],
            velocity,
            yaw,
        })
    }

    /// Sites which must acquire real local terrain before a graphical run can pass.
    pub fn checkpoints(self) -> &'static [[i32; 2]] {
        match self {
            Self::Region512 => &[[255, 255]],
            Self::Region1024 => &[[511, 511]],
            Self::Traverse => &[[4096, -1024]],
            Self::Teleport => &[[65_536, -32_768], [0, 0]],
            Self::LongFlight => &[[8192, -2048]],
            Self::Return => &[[256, 64], [0, 0]],
            Self::Valley
            | Self::Edits
            | Self::Walk
            | Self::Sprint
            | Self::Fly
            | Self::MovingEdits => &[],
        }
    }
}

/// A temporary scripted operator stands immediately above each test column. These
/// 64 commands follow the same reach, collision and mutation checks as a mouse click.
pub struct Burst {
    pub removed: Vec<(Pos, u8)>,
}

impl Burst {
    pub fn remove(
        world: &mut World,
        mut accepted: impl FnMut(&World, Pos, std::time::Instant),
    ) -> Result<Self, String> {
        let mut removed = Vec::with_capacity(64);
        for x in -4..4 {
            for z in 12..20 {
                let y = (1..64)
                    .rev()
                    .find(|&y| world.get([x, y, z]) != 0)
                    .ok_or_else(|| format!("Colonne du scénario absente en ({x}, {z})"))?;
                let position = [x, y, z];
                let block = world.get(position);
                let actor = actor_above(position);
                let started = std::time::Instant::now();
                let target = edit(world, &actor, [0.0, -1.0, 0.0], Edit::Remove)
                    .map_err(|error| format!("Destruction du scénario {position:?} : {error}"))?;
                if target != position {
                    return Err(format!("Cible {target:?} au lieu de {position:?}"));
                }
                accepted(world, target, started);
                removed.push((position, block));
            }
        }
        Ok(Self { removed })
    }

    pub fn restore(
        &self,
        world: &mut World,
        mut accepted: impl FnMut(&World, Pos, std::time::Instant),
    ) -> Result<(), String> {
        for &(position, block) in &self.removed {
            let actor = actor_above(position);
            let started = std::time::Instant::now();
            let target = edit(world, &actor, [0.0, -1.0, 0.0], Edit::Place(block))
                .map_err(|error| format!("Restauration du scénario {position:?} : {error}"))?;
            if target != position {
                return Err(format!("Pose en {target:?} au lieu de {position:?}"));
            }
            accepted(world, target, started);
        }
        Ok(())
    }
}

fn actor_above([x, y, z]: Pos) -> Player {
    Player {
        position: [x as f32 + 0.5, y as f32 + 1.001, z as f32 + 0.5],
        velocity: [0.0; 3],
        grounded: false,
        flying: true,
    }
}

pub fn on_chunk_face(position: Pos) -> bool {
    position
        .into_iter()
        .any(|c| matches!(c.rem_euclid(CHUNK), 0 | 15))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_crosses_both_signs_and_reaches_each_promised_distance() {
        for (scenario, half) in [(Scenario::Region512, 256.0), (Scenario::Region1024, 512.0)] {
            assert!(scenario.flight(20, 521).is_none());
            let first = scenario.flight(220, 521).unwrap();
            assert_eq!(first.position, [0.5 - half, 52.0, 0.5 - half]);
            assert_eq!(
                scenario.flight(520, 521).unwrap().position,
                [half - 0.5, 52.0, half - 0.5]
            );
        }
        assert_eq!(
            Scenario::Traverse.flight(520, 521).unwrap().position,
            [4096.5, 52., -1023.5]
        );
        let far = Scenario::Teleport.flight(220, 521).unwrap();
        assert_eq!(far.position, [65_536.5, 52., -32_767.5]);
        assert_eq!(far.velocity, [0.0; 3]);
        assert_eq!(
            Scenario::Teleport.flight(520, 521).unwrap().position,
            [0.5, 52., 0.5]
        );
    }

    #[test]
    fn burst_edits_64_distinct_surface_cells_across_chunk_faces_and_restores_them() {
        let mut world = World::new(20_261_120);
        let burst = Burst::remove(&mut world, |_, _, _| {}).unwrap();
        assert_eq!(world.edits().len(), 64);
        assert!(
            burst
                .removed
                .iter()
                .any(|(pos, _)| pos[0] == -1 && pos[2] == 16)
        );
        assert!(
            burst
                .removed
                .iter()
                .any(|(pos, _)| pos[0] == 0 && pos[2] == 15)
        );
        burst.restore(&mut world, |_, _, _| {}).unwrap();
        assert!(world.edits().is_empty());
    }
}

/// Independent generated surface for the measured column. Keep one column only;
/// this probe cannot insert blocks or meshes in the world under measurement.
#[derive(Default)]
pub struct GroundProbe {
    column: Option<[i32; 2]>,
    chunks: Vec<primal_voxel::GeneratedChunk>,
}
impl GroundProbe {
    pub fn surface(&mut self, world: &World, x: i32, z: i32) -> Pos {
        let column = [x.div_euclid(CHUNK), z.div_euclid(CHUNK)];
        if self.column != Some(column) {
            self.column = Some(column);
            self.chunks = (0..world.block_bounds().1[1] as i32 / CHUNK)
                .map(|y| {
                    primal_voxel::generate_chunk(world.seed(), [column[0], y, column[1]]).unwrap()
                })
                .collect();
        }
        let y = (0..self.chunks.len() as i32 * CHUNK)
            .rev()
            .find(|&y| {
                let p = [x, y, z];
                world
                    .lookup(p)
                    .or_else(|| self.chunks[(y / CHUNK) as usize].get(p))
                    .is_some_and(|b| b != 0)
            })
            .unwrap_or(0);
        [x, y, z]
    }
}
