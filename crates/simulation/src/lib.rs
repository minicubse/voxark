//! Renderer-independent movement and authoritative voxel interactions.

use primal_voxel::{MAX_BLOCK, Pos, WORLD_MAX, WORLD_MIN, World};
use std::fmt;

pub const FIXED_DT: f32 = 1.0 / 60.0;
pub const MAX_REACH: f32 = 6.0;
pub const PLAYER_HEIGHT: f32 = 1.8;
pub const PLAYER_RADIUS: f32 = 0.3;
const EYE_HEIGHT: f32 = 1.62;
const CONTACT_EPSILON: f32 = 0.0001;
fn movement_bounds(world: &World) -> ([f32; 3], [f32; 3]) {
    let (lo, hi) = world.block_bounds();
    (lo.map(|v| v as f32), hi.map(|v| v as f32))
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    /// World-space movement. Horizontal diagonal input is normalized.
    pub movement: [f32; 3],
    pub jump: bool,
    pub sprint: bool,
}

#[derive(Clone, Debug)]
pub struct Player {
    /// Center of the player's feet in world coordinates.
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub grounded: bool,
    pub flying: bool,
}

impl Player {
    /// Convenience for known playable generated worlds.
    /// Use `try_spawn` when loading arbitrary edited worlds.
    pub fn spawn(world: &World) -> Self {
        Self::try_spawn(world).expect("world has no collision-free player spawn")
    }

    pub fn try_spawn(world: &World) -> Option<Self> {
        // Search expanding squares so a modified spawn retains nearby context.
        let radius_limit = WORLD_MIN[0]
            .abs()
            .max(WORLD_MIN[2].abs())
            .max(WORLD_MAX[0])
            .max(WORLD_MAX[2]);
        for radius in 0..=radius_limit {
            for x in -radius..=radius {
                for z in -radius..=radius {
                    if x.abs().max(z.abs()) != radius {
                        continue;
                    }
                    if !World::contains([x, WORLD_MIN[1] + 1, z]) {
                        continue;
                    }
                    let top = world.block_bounds().1[1] as i32 - PLAYER_HEIGHT.ceil() as i32;
                    for y in ((WORLD_MIN[1] + 1)..=top).rev() {
                        if world.get([x, y - 1, z]) == 0 {
                            continue;
                        }
                        let candidate = Self {
                            position: [x as f32 + 0.5, y as f32 + CONTACT_EPSILON, z as f32 + 0.5],
                            velocity: [0.0; 3],
                            grounded: true,
                            flying: false,
                        };
                        if !candidate.collides(world, candidate.position) {
                            return Some(candidate);
                        }
                    }
                }
            }
        }
        None
    }

    /// Wait for actual cells, or find clear space above an edited arrival point.
    pub fn arrival(world: &World, position: [f32; 3], flying: bool) -> Option<Self> {
        if !finite(position) || !(1.0..=126.0).contains(&position[1]) {
            return None;
        }
        let mut p = Self {
            position,
            velocity: [0.; 3],
            grounded: false,
            flying,
        };
        while p.position[1] <= 126.0 {
            if !p.collision_data_ready(world) {
                return None;
            }
            if !p.collides(world, p.position) {
                return Some(p);
            }
            p.position[1] = p.position[1].floor() + 1.0001;
        }
        None
    }

    pub fn eye(&self) -> [f32; 3] {
        [
            self.position[0],
            self.position[1] + EYE_HEIGHT,
            self.position[2],
        ]
    }

    /// Advance one simulation step. Call at `FIXED_DT` from an accumulator.
    /// Non-finite input is discarded and individual steps are capped at 250 ms.
    pub fn step(&mut self, world: &World, input: Input, dt: f32) {
        if !dt.is_finite()
            || dt <= 0.0
            || !finite(input.movement)
            || !finite(self.position)
            || !finite(self.velocity)
        {
            return;
        }
        let dt = dt.min(0.25);
        let mut movement = input.movement;
        if !self.flying {
            movement[1] = 0.0;
        }
        let movement = normalized_input(movement);
        let speed = match (self.flying, input.sprint) {
            (true, true) => 18.0,
            (true, false) => 9.0,
            (false, true) => 7.0,
            (false, false) => 4.0,
        };
        self.velocity[0] = movement[0] * speed;
        self.velocity[2] = movement[2] * speed;
        if self.flying {
            self.velocity[1] = movement[1] * speed;
        } else {
            if input.jump && self.grounded {
                self.velocity[1] = 8.0;
            }
            self.velocity[1] = (self.velocity[1] - 24.0 * dt).clamp(-40.0, 40.0);
        }
        self.grounded = false;
        // Substeps keep a one-voxel wall detectable at sprint/terminal velocity.
        let maximum = self
            .velocity
            .iter()
            .map(|v| v.abs() * dt)
            .fold(0.0, f32::max);
        let steps = (maximum / 0.2).ceil().max(1.0) as u32;
        let substep_dt = dt / steps as f32;
        for _ in 0..steps {
            for axis in [0, 2, 1] {
                self.move_axis(world, axis, self.velocity[axis] * substep_dt);
            }
        }
    }

    /// All cells touched by the AABB, including support immediately below the feet.
    pub fn collision_data_ready(&self, world: &World) -> bool {
        let (mut lo, hi) = bounds(self.position);
        lo[1] -= 0.02;
        let (lo, hi) = voxel_range(lo, hi);
        (lo[0]..=hi[0]).all(|x| {
            (lo[1]..=hi[1]).all(|y| {
                (lo[2]..=hi[2])
                    .all(|z| !world.in_bounds([x, y, z]) || world.lookup([x, y, z]).is_some())
            })
        })
    }
    fn collides(&self, world: &World, position: [f32; 3]) -> bool {
        let (minimum, maximum) = bounds(position);
        let (lo, hi) = movement_bounds(world);
        if (0..3).any(|axis| minimum[axis] < lo[axis] || maximum[axis] > hi[axis]) {
            return true;
        }
        let (minimum, maximum) = voxel_range(minimum, maximum);
        for x in minimum[0]..=maximum[0] {
            for y in minimum[1]..=maximum[1] {
                for z in minimum[2]..=maximum[2] {
                    if world.lookup([x, y, z]) != Some(0) {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn move_axis(&mut self, world: &World, axis: usize, distance: f32) {
        if distance == 0.0 {
            return;
        }
        let mut candidate = self.position;
        candidate[axis] += distance;
        let (minimum, maximum) = bounds(candidate);
        let positive = distance > 0.0;
        let minimum_offset = if axis == 1 { 0.0 } else { -PLAYER_RADIUS };
        let maximum_offset = if axis == 1 {
            PLAYER_HEIGHT
        } else {
            PLAYER_RADIUS
        };
        let (lo, hi) = movement_bounds(world);
        let mut resolved = candidate[axis].clamp(
            lo[axis] - minimum_offset + CONTACT_EPSILON,
            hi[axis] - maximum_offset - CONTACT_EPSILON,
        );
        let (minimum, maximum) = voxel_range(minimum, maximum);
        for x in minimum[0]..=maximum[0] {
            for y in minimum[1]..=maximum[1] {
                for z in minimum[2]..=maximum[2] {
                    let voxel = [x, y, z];
                    if world.lookup(voxel) == Some(0) {
                        continue;
                    }
                    if positive {
                        resolved =
                            resolved.min(voxel[axis] as f32 - maximum_offset - CONTACT_EPSILON);
                    } else {
                        resolved = resolved
                            .max(voxel[axis] as f32 + 1.0 - minimum_offset + CONTACT_EPSILON);
                    }
                }
            }
        }
        if resolved != candidate[axis] {
            self.velocity[axis] = 0.0;
            if axis == 1 && !positive && !self.flying {
                self.grounded = true;
            }
        }
        self.position[axis] = resolved;
    }
}

fn finite(value: [f32; 3]) -> bool {
    value.iter().all(|number| number.is_finite())
}

fn normalized_input(value: [f32; 3]) -> [f32; 3] {
    // f64 avoids overflow with finite but adversarial f32 input.
    let length = value
        .iter()
        .map(|v| f64::from(*v).powi(2))
        .sum::<f64>()
        .sqrt()
        .max(1.0);
    value.map(|v| (f64::from(v) / length) as f32)
}

fn bounds(position: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    (
        [
            position[0] - PLAYER_RADIUS,
            position[1],
            position[2] - PLAYER_RADIUS,
        ],
        [
            position[0] + PLAYER_RADIUS,
            position[1] + PLAYER_HEIGHT,
            position[2] + PLAYER_RADIUS,
        ],
    )
}

fn voxel_range(minimum: [f32; 3], maximum: [f32; 3]) -> (Pos, Pos) {
    (
        minimum.map(|value| (value + CONTACT_EPSILON * 0.1).floor() as i32),
        maximum.map(|value| (value - CONTACT_EPSILON * 0.1).floor() as i32),
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub block: Pos,
    pub adjacent: Pos,
    pub normal: [i32; 3],
    pub distance: f32,
}

/// Exact voxel traversal. The direction is normalized internally and reach is
/// measured in world units. Boundary/corner ties advance all crossed axes.
pub fn raycast(world: &World, origin: [f32; 3], direction: [f32; 3], reach: f32) -> Option<Hit> {
    if !finite(origin) || !finite(direction) || !reach.is_finite() || reach < 0.0 {
        return None;
    }
    let origin = origin.map(f64::from);
    let direction = direction.map(f64::from);
    let length = direction.iter().map(|v| v * v).sum::<f64>().sqrt();
    if length == 0.0 {
        return None;
    }
    let direction = direction.map(|v| v / length);
    let mut enter = 0.0_f64;
    let mut leave = f64::from(reach);
    let mut normal = [0; 3];
    let (lo, hi) = world.block_bounds();
    for axis in 0..3 {
        let lower = lo[axis] as f64;
        let upper = hi[axis] as f64;
        if direction[axis] == 0.0 {
            if origin[axis] < lower || origin[axis] >= upper {
                return None;
            }
            continue;
        }
        let a = (lower - origin[axis]) / direction[axis];
        let b = (upper - origin[axis]) / direction[axis];
        let near = a.min(b);
        if near > enter {
            enter = near;
            normal = [0; 3];
            normal[axis] = if direction[axis] > 0.0 { -1 } else { 1 };
        }
        leave = leave.min(a.max(b));
    }
    if enter > leave || leave < 0.0 {
        return None;
    }
    let point = std::array::from_fn::<_, 3, _>(|axis| origin[axis] + direction[axis] * enter);
    let mut cell = std::array::from_fn::<_, 3, _>(|axis| {
        let mut cell = point[axis].floor() as i32;
        if direction[axis] < 0.0 && point[axis] == point[axis].floor() {
            cell = cell.saturating_sub(1);
        }
        cell
    });
    if normal == [0; 3] {
        for axis in 0..3 {
            if direction[axis] != 0.0 && point[axis] == point[axis].floor() {
                normal[axis] = if direction[axis] > 0.0 { -1 } else { 1 };
                break;
            }
        }
    }
    let step = direction.map(|v| {
        if v > 0.0 {
            1
        } else if v < 0.0 {
            -1
        } else {
            0
        }
    });
    let delta = direction.map(|v| {
        if v == 0.0 {
            f64::INFINITY
        } else {
            1.0 / v.abs()
        }
    });
    let mut crossing = std::array::from_fn::<_, 3, _>(|axis| {
        if step[axis] == 0 {
            return f64::INFINITY;
        }
        let edge = f64::from(cell[axis]) + f64::from(step[axis] > 0);
        (edge - origin[axis]) / direction[axis]
    });
    let mut distance = enter;
    while world.in_bounds(cell) && distance <= leave {
        if world.lookup(cell)? != 0 {
            return Some(Hit {
                block: cell,
                adjacent: std::array::from_fn(|axis| cell[axis].saturating_add(normal[axis])),
                normal,
                distance: distance as f32,
            });
        }
        distance = crossing.iter().copied().fold(f64::INFINITY, f64::min);
        normal = [0; 3];
        let mut found_normal = false;
        for axis in 0..3 {
            if (crossing[axis] - distance).abs() <= 1e-10 {
                cell[axis] = cell[axis].checked_add(step[axis])?;
                crossing[axis] += delta[axis];
                if !found_normal {
                    normal[axis] = -step[axis];
                    found_normal = true;
                }
            }
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    Remove,
    Place(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditError {
    InvalidPlayer,
    InvalidBlock,
    NoTarget,
    OutOfBounds,
    Occupied,
    PlayerOverlap,
    Unchanged,
}

impl fmt::Display for EditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidPlayer => "invalid player coordinates",
            Self::InvalidBlock => "unknown or empty construction block",
            Self::NoTarget => "no block within reach",
            Self::OutOfBounds => "target lies outside the world",
            Self::Occupied => "target already contains a block",
            Self::PlayerOverlap => "placement overlaps the player",
            Self::Unchanged => "world rejected the edit",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for EditError {}

/// Server-ready validation resolves the target from the player's eye and checks
/// reach, contents and collision before changing the authoritative world.
pub fn edit(
    world: &mut World,
    player: &Player,
    direction: [f32; 3],
    action: Edit,
) -> Result<Pos, EditError> {
    if !finite(player.position) {
        return Err(EditError::InvalidPlayer);
    }
    let (minimum, maximum) = bounds(player.position);
    let (lo, hi) = movement_bounds(world);
    if (0..3).any(|axis| minimum[axis] < lo[axis] || maximum[axis] > hi[axis]) {
        return Err(EditError::InvalidPlayer);
    }
    if matches!(action, Edit::Place(block) if block == 0 || block > MAX_BLOCK) {
        return Err(EditError::InvalidBlock);
    }
    let hit = raycast(world, player.eye(), direction, MAX_REACH).ok_or(EditError::NoTarget)?;
    let (target, block) = match action {
        Edit::Remove => (hit.block, 0),
        Edit::Place(block) => (hit.adjacent, block),
    };
    if !world.in_bounds(target) {
        return Err(EditError::OutOfBounds);
    }
    if block != 0 {
        if world.lookup(target) != Some(0) {
            return Err(EditError::Occupied);
        }
        let (minimum, maximum) = bounds(player.position);
        if (0..3).all(|axis| {
            minimum[axis] < target[axis] as f32 + 1.0 && maximum[axis] > target[axis] as f32
        }) {
            return Err(EditError::PlayerOverlap);
        }
    }
    if !world.set(target, block) {
        return Err(EditError::Unchanged);
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_world() -> World {
        let mut world = World::new(17);
        for x in -5..=5 {
            for z in -5..=5 {
                for y in 35..64 {
                    world.set([x, y, z], 0);
                }
                world.set([x, 39, z], 3);
            }
        }
        world
    }

    fn player() -> Player {
        Player {
            position: [0.5, 40.0, 0.5],
            velocity: [0.0; 3],
            grounded: true,
            flying: false,
        }
    }

    #[test]
    fn falls_onto_voxel_floor_without_penetration() {
        let world = test_world();
        let mut player = player();
        player.position[1] = 48.0;
        player.grounded = false;
        for _ in 0..300 {
            player.step(&world, Input::default(), FIXED_DT);
        }
        assert!((player.position[1] - 40.0).abs() < 0.002);
        assert!(player.grounded);
        assert_eq!(player.velocity[1], 0.0);
    }

    #[test]
    fn jump_rises_then_returns_to_floor() {
        let world = test_world();
        let mut player = player();
        player.step(
            &world,
            Input {
                jump: true,
                ..Input::default()
            },
            FIXED_DT,
        );
        assert!(player.position[1] > 40.0);
        assert!(!player.grounded);
        for _ in 0..180 {
            player.step(&world, Input::default(), FIXED_DT);
        }
        assert!((player.position[1] - 40.0).abs() < 0.002);
    }

    #[test]
    fn sprint_and_diagonal_input_respect_speed() {
        let world = test_world();
        let mut walking = player();
        let mut sprinting = player();
        let mut diagonal = player();
        walking.step(
            &world,
            Input {
                movement: [1.0, 0.0, 0.0],
                ..Input::default()
            },
            FIXED_DT,
        );
        sprinting.step(
            &world,
            Input {
                movement: [1.0, 0.0, 0.0],
                sprint: true,
                ..Input::default()
            },
            FIXED_DT,
        );
        diagonal.step(
            &world,
            Input {
                movement: [1.0, 0.0, 1.0],
                ..Input::default()
            },
            FIXED_DT,
        );
        let distance =
            |p: &Player| ((p.position[0] - 0.5).powi(2) + (p.position[2] - 0.5).powi(2)).sqrt();
        assert!(distance(&sprinting) > distance(&walking));
        assert!((distance(&diagonal) - distance(&walking)).abs() < 0.00001);
    }

    #[test]
    fn fast_flight_stops_at_wall_and_world_boundary() {
        let mut world = test_world();
        for y in 40..44 {
            for z in -2..3 {
                world.set([2, y, z], 3);
            }
        }
        let mut player = player();
        player.flying = true;
        player.step(
            &world,
            Input {
                movement: [1.0, 0.0, 0.0],
                sprint: true,
                ..Input::default()
            },
            0.25,
        );
        assert!(player.position[0] <= 1.701);
        player.position = [63.5, 55.0, 0.5];
        player.step(
            &world,
            Input {
                movement: [1.0, 0.0, 0.0],
                sprint: true,
                ..Input::default()
            },
            0.25,
        );
        assert!(player.position[0] <= 63.701);
    }

    #[test]
    fn raycast_hits_exact_face_and_negative_boundary() {
        let mut world = test_world();
        world.set([0, 41, 3], 3);
        let hit = raycast(&world, [0.5, 41.5, 0.5], [0.0, 0.0, 10.0], 6.0).unwrap();
        assert_eq!(hit.block, [0, 41, 3]);
        assert_eq!(hit.adjacent, [0, 41, 2]);
        assert_eq!(hit.normal, [0, 0, -1]);
        assert!((hit.distance - 2.5).abs() < 0.00001);
        world.set([-1, 41, 0], 3);
        let boundary = raycast(&world, [0.0, 41.5, 0.5], [-1.0, 0.0, 0.0], 6.0).unwrap();
        assert_eq!(boundary.block, [-1, 41, 0]);
        assert_eq!(boundary.distance, 0.0);
    }

    #[test]
    fn raycast_rejects_bad_numbers_and_respects_reach() {
        let mut world = test_world();
        world.set([0, 41, 3], 3);
        assert!(raycast(&world, [0.5, 41.5, 0.5], [0.0; 3], 6.0).is_none());
        assert!(raycast(&world, [f32::NAN, 41.5, 0.5], [0.0, 0.0, 1.0], 6.0).is_none());
        assert!(raycast(&world, [0.5, 41.5, 0.5], [0.0, 0.0, f32::INFINITY], 6.0).is_none());
        assert!(raycast(&world, [0.5, 41.5, 0.5], [0.0, 0.0, 1.0], 2.0).is_none());
        assert!(raycast(&world, [0.5, 41.5, 0.5], [0.0, 0.0, f32::MAX], 6.0).is_some());
    }

    #[test]
    fn edits_remove_and_place_through_validated_ray() {
        let mut world = test_world();
        world.set([0, 41, 3], 3);
        let player = player();
        assert_eq!(
            edit(&mut world, &player, [0.0, 0.0, 1.0], Edit::Place(2)),
            Ok([0, 41, 2])
        );
        assert_eq!(world.get([0, 41, 2]), 2);
        assert_eq!(
            edit(&mut world, &player, [0.0, 0.0, 1.0], Edit::Remove),
            Ok([0, 41, 2])
        );
        assert_eq!(world.get([0, 41, 2]), 0);
    }

    #[test]
    fn edits_reject_self_overlap_unknown_blocks_and_far_targets() {
        let mut world = test_world();
        let player = player();
        world.set([0, 41, 1], 3);
        assert_eq!(
            edit(&mut world, &player, [0.0, 0.0, 1.0], Edit::Place(2)),
            Err(EditError::PlayerOverlap)
        );
        assert_eq!(
            edit(&mut world, &player, [0.0, 0.0, 1.0], Edit::Place(0)),
            Err(EditError::InvalidBlock)
        );
        assert_eq!(
            edit(&mut world, &player, [0.0, 0.0, 1.0], Edit::Place(u8::MAX)),
            Err(EditError::InvalidBlock)
        );
        world.set([0, 41, 1], 0);
        world.set([0, 41, 8], 3);
        assert_eq!(
            edit(&mut world, &player, [0.0, 0.0, 1.0], Edit::Remove),
            Err(EditError::NoTarget)
        );
        assert_eq!(world.get([0, 41, 8]), 3);
    }

    #[test]
    fn spawn_finds_space_above_modified_origin() {
        let mut world = test_world();
        for y in 40..48 {
            world.set([0, y, 0], 3);
        }
        let player = Player::spawn(&world);
        assert!(player.position[1] >= 48.0);
        assert!(!player.collides(&world, player.position));
    }

    #[test]
    fn invalid_input_cannot_poison_player_state() {
        let world = test_world();
        let mut player = player();
        let original = player.position;
        player.step(
            &world,
            Input {
                movement: [f32::NAN, 0.0, 0.0],
                ..Input::default()
            },
            FIXED_DT,
        );
        assert_eq!(player.position, original);
        player.step(&world, Input::default(), f32::INFINITY);
        assert_eq!(player.position, original);
    }

    #[test]
    fn imported_velocity_is_bounded_before_collision_substeps() {
        let world = test_world();
        let mut player = player();
        player.velocity[1] = 1000.0;
        player.step(&world, Input::default(), 0.01);
        assert!(player.position[1] < 40.5);
        assert!(player.velocity[1] <= 40.0);
    }

    #[test]
    fn edits_reject_player_outside_playable_bounds() {
        let mut world = test_world();
        let mut player = player();
        player.position[0] = -64.1;
        assert_eq!(
            edit(&mut world, &player, [1.0, 0.0, 0.0], Edit::Remove),
            Err(EditError::InvalidPlayer)
        );
    }
}
