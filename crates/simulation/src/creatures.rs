//! Deterministic server demonstration creatures, independent of rendering and streaming.

use primal_voxel::{Preset, surface_for};
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;

mod navigation;
mod reactive;
pub use reactive::Decision;
use reactive::Reactive;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Behavior {
    #[default]
    Demo,
    Reactive,
    Navigation,
}

pub const MAX_CREATURES: usize = 12;
const CREATURE_ID_BASE: u64 = 1 << 63;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CreatureId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Species {
    Wyvern,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Palette {
    Jade,
    Braise,
    Obsidienne,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Locomotion {
    IdleGround,
    Walk,
    Takeoff,
    Flap,
    Glide,
    Land,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreatureState {
    pub id: CreatureId,
    pub species: Species,
    pub palette: Palette,
    /// Feet pivot in authoritative world coordinates, in metres.
    pub position: [f64; 3],
    /// Rotation about +Y in radians, with forward at -Z when yaw is zero.
    pub yaw: f32,
    pub locomotion: Locomotion,
    pub state_start_tick: u64,
}

impl CreatureState {
    pub fn within_horizontal(&self, center: [f64; 3], radius: f64) -> bool {
        let dx = self.position[0] - center[0];
        let dz = self.position[2] - center[2];
        radius.is_finite() && dx.hypot(dz) <= radius
    }
}

/// Server-lifetime population. Slots are recreated from configuration on restart;
/// motion is sampled at the current server tick without saved phase or entity data.
pub struct CreatureWorld {
    population: Vec<(CreatureId, usize)>,
    serial: u64,
    tick_hz: u32,
    stage_ends: [u64; STAGES.len()],
    ground: GroundPatch,
    behavior: Behavior,
    reactive: Option<Reactive>,
    last_tick: u64,
}

impl CreatureWorld {
    pub fn new(seed: u64, preset: Preset, count: usize, tick_hz: u32) -> Self {
        Self::with_behavior(seed, preset, count, tick_hz, Behavior::Demo)
    }

    pub fn with_behavior(
        seed: u64,
        preset: Preset,
        count: usize,
        tick_hz: u32,
        behavior: Behavior,
    ) -> Self {
        let tick_hz = tick_hz.max(1);
        let mut cycle = 0;
        let stage_ends = STAGES.map(|stage| {
            cycle += ((stage.centiseconds * u64::from(tick_hz) + 50) / 100).max(1);
            cycle
        });
        let mut world = Self {
            population: Vec::with_capacity(MAX_CREATURES),
            serial: CREATURE_ID_BASE,
            behavior,
            reactive: None,
            last_tick: 0,
            tick_hz,
            stage_ends,
            ground: GroundPatch::new(seed, preset),
        };
        for _ in 0..count.min(MAX_CREATURES) {
            world.spawn();
        }
        world
    }

    /// A free route slot may be reused; its previous network identity never is.
    pub fn spawn(&mut self) -> Option<CreatureId> {
        let slot = (0..MAX_CREATURES)
            .find(|slot| !self.population.iter().any(|entry| entry.1 == *slot))?;
        self.serial = self.serial.checked_add(1)?;
        let id = CreatureId(self.serial);
        self.population.push((id, slot));
        if slot == 0 && self.behavior != Behavior::Demo {
            let mut home = self.state(id, slot, self.motion(0));
            home.state_start_tick = self.last_tick;
            self.reactive = Some(Reactive::new(home, self.behavior == Behavior::Navigation));
        }
        Some(id)
    }

    pub fn despawn(&mut self, id: CreatureId) -> bool {
        let Some(index) = self.population.iter().position(|entry| entry.0 == id) else {
            return false;
        };
        if self.population.remove(index).1 == 0 {
            self.reactive = None;
        }
        true
    }

    /// Advance once after authoritative player physics. Positions are feet, in metres.
    pub fn advance(
        &mut self,
        tick: u64,
        players: impl Iterator<Item = (u64, [f64; 3])>,
        clear: impl Fn([f64; 3]) -> bool,
    ) {
        self.last_tick = tick;
        if let Some(actor) = &mut self.reactive {
            actor.step(tick, self.tick_hz, &self.ground, players, clear);
        }
    }

    /// Invalidate the local path only for edits in the bounded navigation patch.
    pub fn terrain_edited(&mut self, [x, _, z]: [i32; 3]) {
        if (-PATCH_EDGE..PATCH_EDGE).contains(&x)
            && (-PATCH_EDGE..PATCH_EDGE).contains(&z)
            && let Some(nav) = self.reactive.as_mut().and_then(|r| r.navigation.as_mut())
        {
            nav.invalidate();
        }
    }

    pub fn decision(&self) -> Option<(Decision, Option<u64>)> {
        self.reactive.as_ref().map(|r| (r.decision, r.target))
    }

    /// Reactive slots return their last advanced state. Demo slots sample the given tick.
    /// All spawned slots at one authoritative tick, in stable identity order.
    pub fn states(&self, tick: u64) -> impl ExactSizeIterator<Item = CreatureState> + '_ {
        let motion = self.motion(tick);
        self.population
            .iter()
            .map(move |&(id, slot)| self.state(id, slot, motion))
    }

    pub fn sample(&self, id: CreatureId, tick: u64) -> Option<CreatureState> {
        self.population
            .iter()
            .find(|entry| entry.0 == id)
            .map(|&(_, slot)| self.state(id, slot, self.motion(tick)))
    }

    fn motion(&self, tick: u64) -> Motion {
        let cycle_tick = tick % self.stage_ends[STAGES.len() - 1];
        let index = self.stage_ends.partition_point(|&end| end <= cycle_tick);
        let start_tick = if index == 0 {
            0
        } else {
            self.stage_ends[index - 1]
        };
        let elapsed = cycle_tick - start_tick;
        let duration = self.stage_ends[index] - start_tick;
        let fraction = elapsed as f64 / duration as f64;
        let duration_seconds = duration as f64 / f64::from(self.tick_hz);
        let stage = STAGES[index];
        let origin = index.checked_sub(1).map_or(START, |prev| STAGES[prev].end);
        let (angle, angular_speed) =
            interpolate(origin.angle, stage.end.angle, fraction, duration_seconds);
        let (altitude, ascent_speed) = interpolate(
            origin.altitude,
            stage.end.altitude,
            fraction,
            duration_seconds,
        );
        // Expand the turn only once the creature is away from the ground and
        // contract it before descending into the central clearing.
        let expansion = ((altitude - 7.0) / 10.0).clamp(0.0, 1.0);
        let radius = 8.0 + 6.0 * smoothstep(expansion);
        let radial_speed = 3.6 * expansion * (1.0 - expansion) * ascent_speed;
        Motion {
            locomotion: stage.locomotion,
            state_start_tick: tick - elapsed,
            angle,
            tangential_speed: radius * angular_speed,
            radius,
            radial_speed,
            altitude,
        }
    }

    fn state(&self, id: CreatureId, slot: usize, motion: Motion) -> CreatureState {
        if let Some(r) = &self.reactive
            && r.state.id == id
        {
            return r.state;
        }
        // Opposing slots are filled first so 1, 2 or 4 actors fit the same route.
        const SLOT_ORDER: [u8; MAX_CREATURES] = [0, 6, 3, 9, 1, 7, 4, 10, 2, 8, 5, 11];
        const PALETTES: [Palette; 3] = [Palette::Jade, Palette::Braise, Palette::Obsidienne];
        const SPAWN_ANGLE: f64 = 2.0;
        let angle =
            SPAWN_ANGLE + motion.angle + f64::from(SLOT_ORDER[slot]) * TAU / MAX_CREATURES as f64;
        let (sin, cos) = angle.sin_cos();
        let x = 0.5 + motion.radius * cos;
        let z = 0.5 + motion.radius * sin;
        let (dx, dz) = if motion.tangential_speed == 0.0 && motion.radial_speed == 0.0 {
            (-sin, cos)
        } else {
            (
                motion.radial_speed * cos - motion.tangential_speed * sin,
                motion.radial_speed * sin + motion.tangential_speed * cos,
            )
        };
        CreatureState {
            id,
            species: Species::Wyvern,
            palette: PALETTES[slot % PALETTES.len()],
            position: [x, self.ground.at(x, z) + motion.altitude, z],
            yaw: (-dx).atan2(-dz) as f32,
            locomotion: motion.locomotion,
            state_start_tick: motion.state_start_tick,
        }
    }
}

#[derive(Clone, Copy)]
struct Motion {
    locomotion: Locomotion,
    state_start_tick: u64,
    angle: f64,
    tangential_speed: f64,
    radius: f64,
    radial_speed: f64,
    altitude: f64,
}

#[derive(Clone, Copy)]
struct Waypoint {
    /// Pairs of position and velocity at the stage boundary, in world units per second.
    angle: [f64; 2],
    altitude: [f64; 2],
}

impl Waypoint {
    const fn at(angle: [f64; 2], altitude: [f64; 2]) -> Self {
        Self { angle, altitude }
    }
}

#[derive(Clone, Copy)]
struct Stage {
    locomotion: Locomotion,
    centiseconds: u64,
    end: Waypoint,
}

impl Stage {
    const fn new(locomotion: Locomotion, centiseconds: u64, end: Waypoint) -> Self {
        Self {
            locomotion,
            centiseconds,
            end,
        }
    }
}

const START: Waypoint = Waypoint::at([0.0, 0.0], [0.0, 0.0]);
const STAGES: [Stage; 6] = [
    Stage::new(Locomotion::IdleGround, 500, START),
    Stage::new(
        Locomotion::Walk,
        312,
        Waypoint::at([0.525, 0.25], [0.0, 0.0]),
    ),
    Stage::new(
        Locomotion::Takeoff,
        105,
        Waypoint::at([0.94, 0.525], [3.0, 4.0]),
    ),
    Stage::new(
        Locomotion::Flap,
        460,
        Waypoint::at([3.8, 0.48], [20.0, 0.0]),
    ),
    Stage::new(
        Locomotion::Glide,
        520,
        Waypoint::at([TAU - 0.5, 0.6875], [3.0, -4.0]),
    ),
    Stage::new(Locomotion::Land, 110, Waypoint::at([TAU, 0.0], [0.0, 0.0])),
];

/// Cubic Hermite interpolation preserves position and velocity at each stage boundary.
fn interpolate(
    [start, start_speed]: [f64; 2],
    [end, end_speed]: [f64; 2],
    t: f64,
    seconds: f64,
) -> (f64, f64) {
    let t2 = t * t;
    let remaining = 1.0 - t;
    let position =
        start + (end - start) * smoothstep(t) + start_speed * seconds * t * remaining * remaining
            - end_speed * seconds * t2 * remaining;
    let speed = (end - start) / seconds * 6.0 * t * remaining
        + start_speed * (3.0 * t2 - 4.0 * t + 1.0)
        + end_speed * (3.0 * t2 - 2.0 * t);
    (position, speed)
}

fn smoothstep(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

const PATCH_EDGE: i32 = 16;
const PATCH_WIDTH: usize = 2 * PATCH_EDGE as usize + 1;

/// Generated terrain under the fixed route. This demo does not pathfind around edits.
struct GroundPatch([[f64; PATCH_WIDTH]; PATCH_WIDTH]);

impl GroundPatch {
    fn new(seed: u64, preset: Preset) -> Self {
        Self(std::array::from_fn(|z| {
            std::array::from_fn(|x| {
                let x = x as i32 - PATCH_EDGE;
                let z = z as i32 - PATCH_EDGE;
                // Use the highest touching cell at each shared corner, so the
                // smooth surface never dips into a voxel staircase.
                let top = [(x - 1, z - 1), (x, z - 1), (x - 1, z), (x, z)]
                    .into_iter()
                    .map(|(x, z)| surface_for(preset, seed, x, z))
                    .max()
                    .unwrap();
                f64::from(top + 1)
            })
        }))
    }

    fn at(&self, x: f64, z: f64) -> f64 {
        let x_floor = x.floor();
        let z_floor = z.floor();
        let tx = smoothstep(x - x_floor);
        let tz = smoothstep(z - z_floor);
        let ix = (x_floor as i32 + PATCH_EDGE) as usize;
        let iz = (z_floor as i32 + PATCH_EDGE) as usize;
        let near = self.0[iz][ix] + (self.0[iz][ix + 1] - self.0[iz][ix]) * tx;
        let far = self.0[iz + 1][ix] + (self.0[iz + 1][ix + 1] - self.0[iz + 1][ix]) * tx;
        near + (far - near) * tz
    }
}
