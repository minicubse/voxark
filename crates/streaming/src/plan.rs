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
        let lead = [view.velocity[0], view.velocity[2]].map(|v| {
            let limit = config.preload_radius as f32;
            (v * (0.5 / CHUNK as f32)).round().clamp(-limit, limit) as i32
        });
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
    previous: Option<Focus>,
    pub(crate) order: Vec<ChunkPos>,
    ranks: HashMap<ChunkPos, usize>,
    render: HashMap<ChunkPos, usize>,
}

impl Plan {
    pub(crate) fn update(&mut self, view: View, config: &StreamConfig, world: &World) {
        let focus = Focus::new(view, config);
        if self.previous.as_ref() == Some(&focus) {
            return;
        }
        let [cx, cy, cz] = focus.center;
        let [lx, lz] = focus.lead;
        let r = config.load_radius;
        let ahead = r.min(config.preload_radius);
        let mut candidates = Vec::new();
        for dz in (-r).min(lz - ahead)..=r.max(lz + ahead) {
            for dx in (-r).min(lx - ahead)..=r.max(lx + ahead) {
                let horizontal = dx.abs().max(dz.abs());
                let predicted = (dx - lx).abs().max((dz - lz).abs());
                let moving = lx != 0 || lz != 0;
                if horizontal > r && (!moving || predicted > ahead) {
                    continue;
                }
                for y in 0..STREAM_HEIGHT / CHUNK {
                    // Chunk coordinates from i32 block positions have room for the planning halo.
                    let chunk = [cx + dx, y, cz + dz];
                    if !world.contains_chunk(chunk) {
                        continue;
                    }
                    let dy = y - cy;
                    let visible =
                        dx * focus.forward[0] + dy * focus.forward[1] + dz * focus.forward[2] >= 0;
                    let band = if horizontal == 0 {
                        0
                    } else if horizontal <= config.simulation_radius && dy.abs() <= 1 {
                        1
                    } else if moving && predicted <= 1 && dy.abs() <= 1 {
                        2
                    } else if horizontal <= config.render_radius {
                        3
                    } else {
                        5
                    };
                    let distance = dx * dx + dy * dy + dz * dz + i32::from(!visible) * 2;
                    candidates.push((
                        (band, distance, predicted, chunk),
                        horizontal <= config.render_radius,
                    ));
                }
            }
        }
        candidates.sort_unstable();
        self.order.clear();
        self.ranks.clear();
        self.render.clear();
        for ((_, _, _, chunk), render) in candidates.into_iter().take(config.chunk_limit()) {
            let index = self.order.len();
            self.ranks.insert(chunk, index);
            if render {
                self.render.insert(chunk, index);
            }
            self.order.push(chunk);
        }
        self.previous = Some(focus);
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
