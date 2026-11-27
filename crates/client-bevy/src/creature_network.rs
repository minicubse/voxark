//! Bounded, server-timed presentation of the complete creature interest sets.
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    time::Instant,
};

use bevy::{
    camera::visibility::{VisibilitySystems, VisibleEntities},
    math::DVec3,
    prelude::*,
};
use primal_simulation::creatures::{CreatureState, Locomotion, Palette};
use serde_json::json;

use crate::{
    creature_asset::{
        CreatureAnimation, CreatureAssets, CreatureClip, CreatureMesh, CreaturePalette,
        CreatureVisual,
    },
    creature_motion::{FADE, clip_phase, local_position, origin_blocks},
    network::Online,
    terrain::Terrain,
};

const RENDER_DELAY_SECONDS: f64 = 0.1;
const MAX_INTERPOLATION_GAP_SECONDS: f64 = 0.4;
const HISTORY_CAPACITY: usize = 16;

#[derive(Default)]
pub struct NetworkCreatures {
    latest: Option<Snapshot>,
    render_tick: Option<f64>,
    tracks: BTreeMap<u64, Track>,
}

struct Snapshot {
    tick: u64,
    tick_hz: u32,
    received: Instant,
    states: Vec<CreatureState>,
}

impl NetworkCreatures {
    pub fn has_snapshot(&self) -> bool {
        self.latest.is_some()
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn receive(
        &mut self,
        tick: u64,
        tick_hz: u32,
        mut states: Vec<CreatureState>,
        received: Instant,
    ) -> bool {
        if tick_hz == 0
            || self
                .latest
                .as_ref()
                .is_some_and(|snapshot| tick <= snapshot.tick || tick_hz != snapshot.tick_hz)
        {
            return false;
        }
        states.retain(|state| {
            if state.state_start_tick > tick
                || !state.yaw.is_finite()
                || !DVec3::from_array(state.position).is_finite()
            {
                return false;
            }
            let track = self.tracks.entry(state.id.0).or_insert_with(|| Track {
                entered_tick: tick,
                frames: VecDeque::with_capacity(HISTORY_CAPACITY),
            });
            if track.frames.back().is_some_and(|frame| frame.tick == tick) {
                return false;
            }
            if track.frames.len() == HISTORY_CAPACITY {
                track.frames.pop_front();
            }
            track.frames.push_back(Frame {
                tick,
                state: *state,
            });
            true
        });
        // Each frame replaces the interest set. A later appearance starts a fresh history.
        self.tracks
            .retain(|_, track| track.frames.back().is_some_and(|frame| frame.tick == tick));
        self.latest = Some(Snapshot {
            tick,
            tick_hz,
            received,
            states,
        });
        self.render_tick.get_or_insert(tick as f64);
        true
    }

    fn advance(&mut self, now: Instant) -> Option<f64> {
        let snapshot = self.latest.as_ref()?;
        let newest = snapshot.tick as f64;
        let elapsed = now
            .saturating_duration_since(snapshot.received)
            .as_secs_f64();
        let delayed = newest + (elapsed - RENDER_DELAY_SECONDS) * f64::from(snapshot.tick_hz);
        let tick = delayed.clamp(self.render_tick.unwrap_or(newest), newest);
        self.render_tick = Some(tick);
        Some(tick)
    }
}

struct Track {
    entered_tick: u64,
    frames: VecDeque<Frame>,
}

struct Frame {
    tick: u64,
    state: CreatureState,
}

#[derive(Clone, Copy)]
struct ClipTime {
    clip: CreatureClip,
    start_tick: u64,
    elapsed: f64,
}

impl ClipTime {
    fn at(state: &CreatureState, tick: f64, tick_hz: u32) -> Self {
        let clip = match state.locomotion {
            Locomotion::IdleGround => CreatureClip::IdleGround,
            Locomotion::Walk => CreatureClip::Walk,
            Locomotion::Takeoff => CreatureClip::Takeoff,
            Locomotion::Flap => CreatureClip::Flap,
            Locomotion::Glide => CreatureClip::Glide,
            Locomotion::Land => CreatureClip::Land,
        };
        Self {
            clip,
            start_tick: state.state_start_tick,
            elapsed: (tick - state.state_start_tick as f64) / f64::from(tick_hz),
        }
    }
}

#[derive(Component, Clone, Copy)]
struct Pose {
    tick: f64,
    position: DVec3,
    yaw: f32,
    current: ClipTime,
    previous: Option<ClipTime>,
}

impl Track {
    fn sample(&self, tick: f64, tick_hz: u32) -> Pose {
        let first = self
            .frames
            .front()
            .expect("a track contains its first sample");
        let tick = tick.max(first.tick as f64);
        let before = self
            .frames
            .iter()
            .rev()
            .find(|frame| frame.tick as f64 <= tick)
            .unwrap_or(first);
        let max_gap = MAX_INTERPOLATION_GAP_SECONDS * f64::from(tick_hz);
        let after = self.frames.iter().find(|frame| frame.tick as f64 > tick);
        let bracket = after.filter(|frame| (frame.tick - before.tick) as f64 <= max_gap);
        let mut position = DVec3::from_array(before.state.position);
        let mut yaw = before.state.yaw;
        let mut frame = before;
        if let Some(after) = bracket {
            let ratio = (tick - before.tick as f64) / (after.tick - before.tick) as f64;
            position = position.lerp(DVec3::from_array(after.state.position), ratio);
            let delta = after.state.yaw - yaw;
            yaw += delta.sin().atan2(delta.cos()) * ratio as f32;
            if after.state.state_start_tick as f64 <= tick {
                frame = after;
            }
        }
        let current = ClipTime::at(&frame.state, tick, tick_hz);
        let previous = if current.elapsed < FADE.as_secs_f64() {
            self.frames
                .iter()
                .rev()
                .find(|old| {
                    old.tick < current.start_tick
                        && (frame.tick - old.tick) as f64 <= max_gap
                        && old.state.locomotion != frame.state.locomotion
                })
                .map(|old| ClipTime::at(&old.state, tick, tick_hz))
        } else {
            None
        };
        Pose {
            tick,
            position,
            yaw,
            current,
            previous,
        }
    }
}

struct Root {
    entered_tick: u64,
    entity: Entity,
}

#[derive(Resource, Default)]
struct Presentation(BTreeMap<u64, Root>);

pub struct NetworkCreaturePlugin;

impl Plugin for NetworkCreaturePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Presentation>()
            .add_systems(
                Update,
                (publish, animate)
                    .chain()
                    .after(crate::publish)
                    .before(crate::measure),
            )
            .add_systems(
                PostUpdate,
                report
                    .after(bevy::app::AnimationSystems)
                    .after(VisibilitySystems::CheckVisibility),
            );
    }
}

fn publish(
    mut online: ResMut<Online>,
    mut presentation: ResMut<Presentation>,
    terrain: Res<Terrain>,
    mut commands: Commands,
) {
    let replica = &mut online.0.get_mut().unwrap().creatures;
    let tick = replica.advance(Instant::now());
    presentation.0.retain(|id, root| {
        let keep = replica
            .tracks
            .get(id)
            .is_some_and(|track| track.entered_tick == root.entered_tick);
        if !keep {
            commands.entity(root.entity).despawn();
        }
        keep
    });
    let (Some(tick), Some(snapshot)) = (tick, &replica.latest) else {
        return;
    };
    for state in &snapshot.states {
        let id = state.id.0;
        let track = &replica.tracks[&id];
        let pose = track.sample(tick, snapshot.tick_hz);
        let transform = Transform::from_translation(local_position(pose.position, terrain.origin))
            .with_rotation(Quat::from_rotation_y(pose.yaw));
        if let Some(root) = presentation.0.get(&id) {
            commands.entity(root.entity).insert((pose, transform));
        } else {
            let palette = match state.palette {
                Palette::Jade => CreaturePalette::Jade,
                Palette::Braise => CreaturePalette::Braise,
                Palette::Obsidienne => CreaturePalette::Obsidienne,
            };
            let entity = commands
                .spawn((CreatureVisual::new(palette), pose, transform))
                .id();
            presentation.0.insert(
                id,
                Root {
                    entered_tick: track.entered_tick,
                    entity,
                },
            );
        }
    }
}

fn animate(
    assets: Res<CreatureAssets>,
    poses: Query<&Pose>,
    mut players: Query<(&CreatureAnimation, &mut AnimationPlayer)>,
) {
    for (binding, mut player) in &mut players {
        let Ok(pose) = poses.get(binding.owner) else {
            continue;
        };
        let weight = pose
            .previous
            .map_or(1., |_| (pose.current.elapsed / FADE.as_secs_f64()) as f32);
        for clip in CreatureClip::ALL {
            let (Some(node), Some(duration)) = (assets.node(clip), assets.duration(clip)) else {
                continue;
            };
            let active = if pose.current.clip == clip {
                Some((pose.current, weight))
            } else {
                pose.previous
                    .filter(|time| time.clip == clip)
                    .map(|time| (time, 1. - weight))
            };
            if let Some((time, weight)) = active {
                let phase = clip_phase(clip, time.elapsed, f64::from(duration)) as f32;
                // Sample against the replicated clock, even when the camera culled this rig.
                player
                    .play(node)
                    .set_weight(weight)
                    .set_seek_time(phase)
                    .pause();
            } else {
                player.stop(node);
            }
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "Independent ECS queries for the opt-in playtest report"
)]
fn report(
    mut online: ResMut<Online>,
    presentation: Res<Presentation>,
    assets: Res<CreatureAssets>,
    terrain: Res<Terrain>,
    roots: Query<(&Pose, &Transform, &CreatureVisual)>,
    players: Query<(&CreatureAnimation, &AnimationPlayer)>,
    parts: Query<&CreatureMesh>,
    camera: Single<&VisibleEntities, With<Camera3d>>,
) {
    let session = online.0.get_mut().unwrap();
    if !session.take_creature_report() {
        return;
    }
    let replica = &session.creatures;
    let origin = origin_blocks(terrain.origin);
    let visible: HashSet<_> = camera
        .get(std::any::TypeId::of::<Mesh3d>())
        .iter()
        .filter_map(|e| parts.get(*e).ok().map(|part| part.owner))
        .collect();
    let players: HashMap<_, _> = players
        .iter()
        .map(|(binding, player)| (binding.owner, player))
        .collect();
    let visuals: Vec<_> = presentation
        .0
        .iter()
        .filter_map(|(&id, root)| {
            let (pose, transform, visual) = roots.get(root.entity).ok()?;
            let phase = assets
                .node(pose.current.clip)
                .and_then(|node| players.get(&root.entity)?.animation(node))
                .map(|animation| animation.seek_time());
            let forward = transform.forward();
            let yaw = (-forward.x).atan2(-forward.z);
            Some(json!({
                "id": id, "entity": root.entity.to_bits(), "sample_tick": pose.tick,
                "position": (transform.translation.as_dvec3() + origin).to_array(),
                "local_position": transform.translation.to_array(), "yaw": yaw,
                "clip": pose.current.clip, "state_start_tick": pose.current.start_tick,
                "phase_seconds": phase, "ready": visual.is_ready() && phase.is_some(),
                "visible": visible.contains(&root.entity)
            }))
        })
        .collect();
    let empty = Vec::new();
    let result = json!({
        "player_id": session.player_id(), "tick": replica.latest.as_ref().map(|frame| frame.tick),
        "tick_hz": replica.latest.as_ref().map(|frame| frame.tick_hz), "render_tick": replica.render_tick,
        "origin": terrain.origin, "origin_blocks": origin.to_array(),
        "authoritative": replica.latest.as_ref().map_or(&empty, |frame| &frame.states), "visuals": visuals,
    });
    println!("CREATURES {result}");
}

#[cfg(test)]
mod tests;
