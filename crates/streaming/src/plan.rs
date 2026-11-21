use crate::{StreamConfig, View};
use primal_voxel::{CHUNK, ChunkPos, STREAM_HEIGHT, World};
use std::collections::HashMap;

#[derive(Eq, PartialEq)]
struct Focus {
    center: ChunkPos,
    forward: [i32; 3],
    lead: [i32; 2],
}

impl Focus {
    fn new(view: View, config: &StreamConfig) -> Self {
        let forward = view.forward.map(|v| if v.is_finite() { v } else { 0. });
        let length = forward.iter().map(|v| v * v).sum::<f32>().sqrt();
        let forward = if length > 0. {
            forward.map(|v| (v / length * 8.).round() as i32)
        } else {
            [0; 3]
        };
        // Half a second of velocity, clamped before conversion even for extreme flight.
        let motion = [view.velocity[0], view.velocity[2]].map(|v| {
            if v.is_finite() {
                v * (0.5 / CHUNK as f32)
            } else {
                0.
            }
        });
        let scale = (config.preload_radius as f32
            / motion[0].abs().max(motion[1].abs()).max(0.0001))
        .min(1.);
        let lead = motion.map(|v| (v * scale).round() as i32);
        let mut center = view.block.map(|v| v.div_euclid(CHUNK));
        center[1] = center[1].clamp(0, STREAM_HEIGHT / CHUNK - 1);
        Self {
            center,
            forward,
            lead,
        }
    }
}

#[derive(Default)]
pub(crate) struct Plan {
    previous: Vec<Focus>,
    pub(crate) order: Vec<ChunkPos>,
    ranks: HashMap<ChunkPos, usize>,
    render: HashMap<ChunkPos, usize>,
}

impl Plan {
    #[cfg(test)]
    pub(crate) fn update(&mut self, view: View, config: &StreamConfig, world: &World) -> bool {
        self.update_views(&[view], config, world)
    }

    pub(crate) fn update_views(
        &mut self,
        views: &[View],
        config: &StreamConfig,
        world: &World,
    ) -> bool {
        if self.previous.len() == views.len()
            && self
                .previous
                .iter()
                .zip(views)
                .all(|(old, &v)| *old == Focus::new(v, config))
        {
            return false;
        }
        let focuses: Vec<_> = views.iter().map(|&v| Focus::new(v, config)).collect();
        let lists: Vec<_> = focuses
            .iter()
            .map(|focus| self.candidates(focus, config, world))
            .collect();
        self.order.clear();
        self.ranks.clear();
        self.render.clear();
        // Interleave authorities before enforcing the shared budget; overlapping positions share work.
        let max = lists.iter().map(Vec::len).max().unwrap_or(0);
        for i in 0..max {
            for list in &lists {
                let Some(&c) = list.get(i) else { continue };
                if !self.ranks.contains_key(&c) {
                    let rank = self.order.len();
                    self.ranks.insert(c, rank);
                    self.order.push(c);
                    if focuses.iter().any(|f| {
                        (c[0] - f.center[0]).abs().max((c[2] - f.center[2]).abs())
                            <= config.render_radius
                    }) {
                        self.render.insert(c, rank);
                    }
                    if self.order.len() == config.chunk_limit() {
                        break;
                    }
                }
            }
            if self.order.len() == config.chunk_limit() {
                break;
            }
        }
        self.previous = focuses;
        true
    }

    fn candidates(&self, focus: &Focus, config: &StreamConfig, world: &World) -> Vec<ChunkPos> {
        let [cx, cy, cz] = focus.center;
        let [lx, lz] = focus.lead;
        let r = config.load_radius;
        let extent = lx.abs().max(lz.abs());
        // Sample the segment, not the entire square around its endpoint.
        let path: Vec<_> = (0..=extent)
            .map(|i| {
                [
                    (lx * i) as f32 / extent.max(1) as f32,
                    (lz * i) as f32 / extent.max(1) as f32,
                ]
            })
            .collect();
        let mut candidates = Vec::new();
        let rim = i32::from(r >= 2);
        for dz in (-r - rim).min(lz - 1)..=(r + rim).max(lz + 1) {
            for dx in (-r - rim).min(lx - 1)..=(r + rim).max(lx + 1) {
                let horizontal = dx.abs().max(dz.abs());
                let lane = path
                    .iter()
                    .map(|p| (dx as f32 - p[0]).abs().max((dz as f32 - p[1]).abs()))
                    .fold(f32::MAX, f32::min);
                let ahead = extent > 0 && lane <= 1.1;
                let retained = horizontal > r && !ahead;
                if retained && (rim == 0 || horizontal > r + rim) {
                    continue;
                }
                let x = (i64::from(cx + dx) * 16 + 8)
                    .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
                let z = (i64::from(cz + dz) * 16 + 8)
                    .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
                let surface = primal_voxel::surface_for(world.preset(), world.seed(), x, z) / CHUNK;
                for y in 0..STREAM_HEIGHT / CHUNK {
                    // Chunk coordinates from i32 block positions have room for the planning halo.
                    let chunk = [cx + dx, y, cz + dz];
                    if !world.contains_chunk(chunk) {
                        continue;
                    }
                    let dy = y - cy;
                    let visible =
                        dx * focus.forward[0] + dy * focus.forward[1] + dz * focus.forward[2] >= 0;
                    let layer = (y - surface).abs();
                    // Current body/support; immediate and predicted terrain; surrounding halo; background.
                    let band = if retained {
                        if !world.is_resident(chunk) {
                            continue;
                        }
                        8
                    } else if horizontal == 0 && dy == 0 {
                        0
                    } else if horizontal <= config.simulation_radius.min(1) && dy.abs() <= 1 {
                        1
                    } else if (horizontal <= 1 || ahead) && layer <= 1 {
                        2
                    } else if ahead && dy.abs() <= 1 {
                        3
                    } else if horizontal == 0 {
                        4
                    } else if horizontal <= config.simulation_radius && dy.abs() <= 1 {
                        5
                    } else if horizontal <= config.render_radius {
                        6
                    } else {
                        7
                    };
                    let keep = i32::from(self.ranks.contains_key(&chunk));
                    let distance =
                        2 * (dx * dx + dz * dz + dy * dy) + 2 * i32::from(!visible) - keep;
                    let depth = if band == 2 { layer } else { 0 };
                    candidates.push(((band, depth, distance, chunk), chunk));
                }
            }
        }
        candidates.sort_unstable();
        candidates
            .into_iter()
            .take(config.chunk_limit())
            .map(|(_, c)| c)
            .collect()
    }
    pub(crate) fn wants(&self, chunk: ChunkPos) -> bool {
        self.ranks.contains_key(&chunk)
    }

    pub(crate) fn render_rank(&self, chunk: ChunkPos) -> Option<usize> {
        self.render.get(&chunk).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_keep_velocity_slope_and_revoke_abandoned_teleport_interest() {
        let w = World::streamed(42);
        let cfg = StreamConfig {
            load_radius: 2,
            render_radius: 2,
            preload_radius: 8,
            ..Default::default()
        };
        let mut plan = Plan::default();
        let mut v = View {
            block: [0, 52, 0],
            forward: [1., 0., 0.],
            velocity: [512., 0., -128.],
        };
        assert_eq!(Focus::new(v, &cfg).lead, [8, -2]);
        plan.update(v, &cfg, &w);
        assert!(plan.wants([8, 2, -2]));
        assert!(!plan.wants([8, 2, -8]));
        v.velocity = [-512., 0., 128.];
        plan.update(v, &cfg, &w);
        assert!(plan.wants([-8, 2, 2]));
        assert!(!plan.wants([8, 2, -2]));
        v.block[0] = 65_536;
        plan.update(v, &cfg, &w);
        assert!(!plan.wants([0, 3, 0]));
    }

    #[test]
    fn view_and_velocity_prioritize_visible_ground_and_future_position() {
        let world = World::streamed(42);
        let config = StreamConfig {
            load_radius: 2,
            render_radius: 2,
            simulation_radius: 0,
            preload_radius: 4,
            ..Default::default()
        };
        let mut plan = Plan::default();
        let mut view = View {
            block: [0, 32, 0],
            forward: [1., 0., 0.],
            velocity: [0.; 3],
        };
        plan.update(view, &config, &world);
        assert_eq!(plan.order[0], [0, 2, 0]);
        assert!(plan.render_rank([2, 2, 0]) < plan.render_rank([-2, 2, 0]));
        assert!(!plan.wants([5, 2, 0]));
        view.velocity[0] = 500.;
        plan.update(view, &config, &world);
        assert!(plan.wants([5, 2, 0]));
        assert_eq!(plan.render_rank([5, 2, 0]), None);
        assert!(plan.ranks[&[4, 2, 0]] < plan.ranks[&[-2, 2, 0]]);
    }
}
