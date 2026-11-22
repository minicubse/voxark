use std::{any::TypeId, collections::HashMap, time::Duration};

use bevy::{
    animation::{AnimatedBy, RepeatAnimation, transition::AnimationTransitions},
    camera::visibility::{VisibilitySystems, VisibleEntities},
    math::DVec3,
    prelude::*,
    time::{Real, Virtual},
};
use serde_json::{Value, json};

use crate::{
    Bench, CameraRig, LocalWorld, Options,
    creature_asset::{
        CreatureAnimation, CreatureAssets, CreatureClip, CreatureMesh, CreaturePalette,
        CreatureVisual,
    },
    terrain::Terrain,
};

const FADE: Duration = Duration::from_millis(220);
const PROBE_AWAY: f64 = 0.8;
const PROBE_RETURN: f64 = 3.6;
const INSPECTION_KEYS: [KeyCode; 6] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
];

pub(crate) struct CreatureMotionPlugin;

impl Plugin for CreatureMotionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MotionReport>()
            .add_systems(Startup, setup)
            .add_systems(Update, motion_controls.before(update_demo))
            .add_systems(
                PostUpdate,
                record_probe
                    .after(bevy::app::AnimationSystems)
                    .after(VisibilitySystems::CheckVisibility)
                    .run_if(probe_enabled),
            );
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Selection {
    Sequence,
    Clip(CreatureClip),
}

impl Selection {
    fn parse(name: &str) -> Self {
        match name {
            "idleground" => Self::Clip(CreatureClip::IdleGround),
            "walk" => Self::Clip(CreatureClip::Walk),
            "takeoff" => Self::Clip(CreatureClip::Takeoff),
            "flap" => Self::Clip(CreatureClip::Flap),
            "glide" => Self::Clip(CreatureClip::Glide),
            "land" => Self::Clip(CreatureClip::Land),
            _ => Self::Sequence,
        }
    }

    fn start_for(self, elapsed: f64, slot: usize) -> f64 {
        match self {
            Self::Clip(clip) if looping(clip) => elapsed + slot as f64 * 0.137,
            _ => elapsed,
        }
    }

    fn height(self, formation_height: f64) -> f64 {
        match self {
            Self::Clip(CreatureClip::Flap) => formation_height + 1.35,
            Self::Clip(CreatureClip::Glide) => formation_height + 0.85,
            _ => 0.,
        }
    }
}

#[derive(Resource)]
pub(crate) struct CreatureDemo {
    selection: Selection,
    initial_elapsed: f64,
    revision: u64,
    hard_reset: bool,
    ready: bool,
    paused: bool,
    speed: f32,
    probe_elapsed: f64,
    probe_yaw: Option<f32>,
}

#[derive(Component)]
pub(crate) struct Actor {
    slot: usize,
    anchor: DVec3,
    formation_height: f64,
    position: DVec3,
    heading: Vec3,
    inspection_height: [f64; 2],
    elapsed: f64,
    clip: CreatureClip,
    phase: f64,
    revision: u64,
    playing: Option<(CreatureClip, u64)>,
    resume_native: bool,
}

fn setup(
    mut commands: Commands,
    o: Res<Options>,
    world: Option<Res<LocalWorld>>,
    mut virtual_time: ResMut<Time<Virtual>>,
) {
    let seed = world.as_ref().map_or(20_261_120, |local| {
        local.stream.lock().unwrap().world().seed()
    });
    let preset = o.preset.unwrap_or(primal_voxel::Preset::ForestModern);
    let selection = Selection::parse(&o.creature_state);
    let initial_elapsed = o.creature_time.unwrap_or(0.);
    commands.insert_resource(CreatureDemo {
        selection,
        initial_elapsed,
        revision: 1,
        hard_reset: true,
        ready: false,
        paused: o.creature_pause,
        speed: o.creature_speed,
        probe_elapsed: 0.,
        probe_yaw: None,
    });
    virtual_time.set_relative_speed(o.creature_speed);
    virtual_time.pause();
    for slot in 0..o.creatures {
        let row = (slot / 3) as f64;
        let lane = [0., -7., 7.][slot % 3];
        let (x, z, elevation) = match o.creature_placement.as_str() {
            "offscreen" => (24. + row * 7., lane, row * 3.8),
            "far" => (-44. - row * 7., lane, 24. + row * 3.8),
            _ => (-10. - row * 7., lane, row * 3.8),
        };
        let ground = f64::from(primal_voxel::surface_for(preset, seed, x as i32, z as i32) + 1);
        let anchor = DVec3::new(x, ground, z);
        let position = anchor + DVec3::Y * selection.height(elevation);
        let palette = [
            CreaturePalette::Jade,
            CreaturePalette::Braise,
            CreaturePalette::Obsidienne,
        ][slot % 3];
        commands.spawn((
            Name::new(format!("Wyverne {}", slot + 1)),
            Actor {
                slot,
                anchor,
                formation_height: elevation,
                position,
                heading: Vec3::NEG_Z,
                inspection_height: [position.y; 2],
                elapsed: selection.start_for(initial_elapsed, slot),
                clip: CreatureClip::IdleGround,
                phase: 0.,
                revision: 0,
                playing: None,
                resume_native: false,
            },
            CreatureVisual::new(palette),
            Transform::from_translation(position.as_vec3()),
        ));
    }
    println!(
        "Outil de développement · 0 séquence · 1 repos · 2 marche · 3 décollage · 4 battement · 5 plané · 6 atterrissage · P pause · R réinitialiser · [ ] vitesse"
    );
}

fn motion_controls(
    keys: Res<ButtonInput<KeyCode>>,
    o: Res<Options>,
    demo: Option<ResMut<CreatureDemo>>,
    mut virtual_time: ResMut<Time<Virtual>>,
) {
    let Some(mut demo) = demo else { return };
    if o.frames != 0 {
        return;
    }
    let selected = if keys.just_pressed(KeyCode::Digit0) {
        Some(Selection::Sequence)
    } else {
        INSPECTION_KEYS
            .iter()
            .position(|key| keys.just_pressed(*key))
            .map(|index| Selection::Clip(CreatureClip::ALL[index]))
    };
    if keys.just_pressed(KeyCode::KeyP) {
        demo.paused = !demo.paused;
    }
    if selected.is_some() || keys.just_pressed(KeyCode::KeyR) {
        demo.hard_reset = keys.just_pressed(KeyCode::KeyR)
            || selected == Some(Selection::Sequence)
            || demo.paused;
        demo.selection = selected.unwrap_or(demo.selection);
        demo.initial_elapsed = 0.;
        demo.revision += 1;
    }
    let speed_step = match (
        keys.just_pressed(KeyCode::BracketLeft),
        keys.just_pressed(KeyCode::BracketRight),
    ) {
        (true, false) => -0.5,
        (false, true) => 0.5,
        _ => 0.,
    };
    if speed_step != 0. {
        demo.speed = (demo.speed * 2_f32.powf(speed_step)).clamp(0.05, 8.);
        virtual_time.set_relative_speed(demo.speed);
        println!("Vitesse d’animation ×{:.2}", demo.speed);
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "Paramètres indépendants du système Bevy"
)]
pub(crate) fn update_demo(
    o: Res<Options>,
    bench: Res<Bench>,
    terrain: Res<Terrain>,
    demo: Option<ResMut<CreatureDemo>>,
    assets: Option<Res<CreatureAssets>>,
    real: Res<Time<Real>>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut rig: ResMut<CameraRig>,
    mut actors: Query<&mut Actor>,
    mut players: Query<(
        &CreatureAnimation,
        &mut AnimationPlayer,
        &mut AnimationTransitions,
    )>,
) {
    let (Some(mut demo), Some(assets)) = (demo, assets) else {
        return;
    };
    let mut lengths = [0.; 6];
    for (index, clip) in CreatureClip::ALL.into_iter().enumerate() {
        let Some(length) = assets.duration(clip) else {
            return;
        };
        lengths[index] = f64::from(length);
    }
    if !demo.ready {
        demo.ready = assets.counts().players == o.creatures
            && if o.frames == 0 {
                terrain.visible > 0
            } else {
                bench.warm >= 120
            };
    }

    if demo.ready && !demo.paused {
        virtual_time.unpause();
    } else {
        virtual_time.pause();
    }
    if (o.creature_probe || o.creature_record) && demo.ready {
        let before = demo.probe_elapsed;
        demo.probe_elapsed += real.delta_secs_f64();
        if o.creature_probe && before < PROBE_AWAY && demo.probe_elapsed >= PROBE_AWAY {
            demo.probe_yaw = Some(rig.yaw);
            rig.yaw += std::f32::consts::PI;
        }
        if demo.probe_elapsed >= PROBE_RETURN
            && let Some(yaw) = demo.probe_yaw.take()
        {
            rig.yaw = yaw;
        }
    }
    // The generic frame delta used by natively scheduled animations is derived from this
    // virtual clock in First, so the pose and trajectory observe the same frame interval.
    let delta = virtual_time.delta_secs_f64();
    for mut actor in &mut actors {
        let restarting = actor.revision != demo.revision;
        if restarting {
            actor.elapsed = demo.selection.start_for(demo.initial_elapsed, actor.slot);
            actor.revision = demo.revision;
            let stage_height = actor.anchor.y + demo.selection.height(actor.formation_height);
            if demo.hard_reset || actor.playing.is_none() {
                actor.position = actor.anchor.with_y(stage_height);
                actor.heading = Vec3::NEG_Z;
            }
            // In-place selected clips keep world X/Z. Near the ground, raise the actor just
            // enough for the wing stroke, and ease that offset away for a return to rest.
            let current_height = actor.position.y;
            let inspection_height = if (stage_height - current_height).abs() <= 2. {
                stage_height
            } else {
                current_height
            };
            actor.inspection_height = [current_height, inspection_height];
        } else {
            actor.elapsed += delta;
        }
        match demo.selection {
            Selection::Sequence => {
                let sample = sequence_sample(actor.elapsed, lengths);
                actor.clip = sample.clip;
                actor.phase = sample.phase;
                actor.position = actor.anchor + sample.offset;
                actor.heading = sample.heading;
            }
            Selection::Clip(clip) => {
                actor.clip = clip;
                actor.phase = actor.elapsed;
                let [start, end] = actor.inspection_height;
                let elapsed =
                    actor.elapsed - demo.selection.start_for(demo.initial_elapsed, actor.slot);
                let t = (elapsed / FADE.as_secs_f64()).clamp(0., 1.);
                actor.position.y = start + (end - start) * (t * t * (3. - 2. * t));
            }
        }
    }
    for (binding, mut player, mut transitions) in &mut players {
        let Ok(mut actor) = actors.get_mut(binding.owner) else {
            continue;
        };
        if actor.resume_native {
            if let Some((clip, _)) = actor.playing
                && let Some(node) = assets.node(clip)
                && let Some(active) = player.animation_mut(node)
            {
                active.resume();
            }
            actor.resume_native = false;
        }
        let current = (actor.clip, actor.revision);
        if actor.playing == Some(current) {
            continue;
        }
        let Some(node) = assets.node(actor.clip) else {
            continue;
        };
        let hard = actor.playing.is_none()
            || (actor
                .playing
                .is_some_and(|(_, revision)| revision != actor.revision)
                && demo.hard_reset);
        if hard {
            player.stop_all();
            *transitions = AnimationTransitions::new();
        }
        let stride = lengths[clip_index(actor.clip)];
        let phase = clip_phase(actor.clip, actor.phase, stride);
        // The new clip starts at the exact fraction of this frame since the state change.
        // Suppress its first native tick so it cannot consume that frame twice.
        transitions
            .play(&mut player, node, if hard { Duration::ZERO } else { FADE })
            .set_repeat(if looping(actor.clip) {
                RepeatAnimation::Forever
            } else {
                RepeatAnimation::Never
            })
            .set_seek_time(phase as f32)
            .pause();
        actor.playing = Some(current);
        actor.resume_native = true;
    }
}

/// Explicit validation export only. Interactive inspection never installs this camera system.
pub(crate) fn film_camera(
    o: Res<Options>,
    bench: Res<Bench>,
    actors: Query<&Actor>,
    mut rig: ResMut<CameraRig>,
    mut commands: Commands,
) {
    let Ok(actor) = actors.single() else { return };
    let target = actor.position + DVec3::Y * 1.45;
    rig.eye = target + DVec3::new(8.0, 2.4, 3.8);
    let dir = (target - rig.eye).normalize();
    rig.yaw = dir.x.atan2(dir.z) as f32;
    rig.pitch = dir.y.asin() as f32;
    if bench.warm >= 120
        && bench.samples.len() < o.frames
        && let Some(dir) = &o.creature_film
    {
        let path = dir.join(format!("frame-{:04}.png", bench.samples.len()));
        use bevy::render::view::screenshot::{Screenshot, save_to_disk};
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
}
pub(crate) fn publish_actors(terrain: Res<Terrain>, mut actors: Query<(&Actor, &mut Transform)>) {
    for (actor, mut transform) in &mut actors {
        let position = local_position(actor.position, terrain.origin);
        let rotation = Transform::IDENTITY
            .looking_to(actor.heading, Vec3::Y)
            .rotation;
        if transform.translation != position || transform.rotation != rotation {
            transform.translation = position;
            transform.rotation = rotation;
        }
    }
}

fn origin_blocks(origin: [i32; 3]) -> DVec3 {
    DVec3::from_array(origin.map(|index| f64::from(index) * f64::from(primal_voxel::CHUNK as u32)))
}

fn local_position(world: DVec3, origin: [i32; 3]) -> Vec3 {
    (world - origin_blocks(origin)).as_vec3()
}

fn clip_index(clip: CreatureClip) -> usize {
    match clip {
        CreatureClip::IdleGround => 0,
        CreatureClip::Walk => 1,
        CreatureClip::Takeoff => 2,
        CreatureClip::Flap => 3,
        CreatureClip::Glide => 4,
        CreatureClip::Land => 5,
    }
}

fn looping(clip: CreatureClip) -> bool {
    !matches!(clip, CreatureClip::Takeoff | CreatureClip::Land)
}

fn clip_phase(clip: CreatureClip, elapsed: f64, duration: f64) -> f64 {
    if looping(clip) {
        elapsed.rem_euclid(duration)
    } else {
        elapsed.min(duration)
    }
}

fn stage_lengths(clips: [f64; 6]) -> [f64; 6] {
    // Rest, three full strides, push off, climb above the crowns, glide back and touch down.
    [1.2, 3. * clips[1], clips[2], 4.6, 5.2, clips[5]]
}

#[derive(Clone, Copy)]
struct Waypoint {
    offset: DVec3,
    velocity: DVec3,
}

impl Waypoint {
    const fn at(position: [f64; 3], velocity: [f64; 3]) -> Self {
        Self {
            offset: DVec3::from_array(position),
            velocity: DVec3::from_array(velocity),
        }
    }
}

const START: Waypoint = Waypoint::at([0., 0., 0.], [0., 0., 0.]);
const WALK_END: Waypoint = Waypoint::at([0., 0., -4.2], [0., 0., -2.]);
const TAKEOFF_END: Waypoint = Waypoint::at([0., 4., -7.5], [-1.8, 5., -4.]);
const CLIMB_TOP: Waypoint = Waypoint::at([-14., 33., -32.], [-10., 4., -2.]);
const FLAP_END: Waypoint = Waypoint::at([-29., 34., -21.], [0., -0.3, 9.]);
const GLIDE_TURN: Waypoint = Waypoint::at([-12., 29., 12.], [9., -3., 3.]);
const GLIDE_END: Waypoint = Waypoint::at([3., 3.8, 7.], [0., -5.7, -7.]);
const END: Waypoint = Waypoint::at([3., 0., 1.5], [0., 0., 0.]);

struct Sample {
    clip: CreatureClip,
    phase: f64,
    offset: DVec3,
    heading: Vec3,
}

fn sequence_sample(mut time: f64, durations: [f64; 6]) -> Sample {
    let stages = stage_lengths(durations);
    for (clip, length) in CreatureClip::ALL.into_iter().zip(stages) {
        if time < length {
            use CreatureClip::{Flap, Glide, IdleGround, Land, Takeoff, Walk};
            let pose = match clip {
                IdleGround => (START.offset, Vec3::NEG_Z),
                Walk => travel(START, WALK_END, time, length),
                Takeoff => travel(WALK_END, TAKEOFF_END, time, length),
                Flap => arc(TAKEOFF_END, CLIMB_TOP, FLAP_END, time, length, 0.55),
                Glide => arc(FLAP_END, GLIDE_TURN, GLIDE_END, time, length, 0.53),
                Land => travel(GLIDE_END, END, time, length),
            };
            return Sample {
                clip,
                phase: time,
                offset: pose.0,
                heading: pose.1,
            };
        }
        time -= length;
    }
    Sample {
        clip: CreatureClip::IdleGround,
        phase: time,
        offset: END.offset,
        heading: Vec3::NEG_Z,
    }
}

fn arc(
    start: Waypoint,
    middle: Waypoint,
    end: Waypoint,
    time: f64,
    length: f64,
    split: f64,
) -> (DVec3, Vec3) {
    let split = length * split;
    if time < split {
        travel(start, middle, time, split)
    } else {
        travel(middle, end, time - split, length - split)
    }
}

// Cubic Hermite interpolation carries the endpoint velocities through each clip boundary.
fn travel(a: Waypoint, b: Waypoint, time: f64, length: f64) -> (DVec3, Vec3) {
    let t = time / length;
    let t2 = t * t;
    let t3 = t2 * t;
    let a_tangent = a.velocity * length;
    let b_tangent = b.velocity * length;
    let offset = a.offset * (2. * t3 - 3. * t2 + 1.)
        + a_tangent * (t3 - 2. * t2 + t)
        + b.offset * (-2. * t3 + 3. * t2)
        + b_tangent * (t3 - t2);
    let velocity = (a.offset - b.offset) * (6. * t2 - 6. * t)
        + a_tangent * (3. * t2 - 4. * t + 1.)
        + b_tangent * (3. * t2 - 2. * t);
    let heading = Vec3::new(velocity.x as f32, 0., velocity.z as f32)
        .try_normalize()
        .unwrap_or(Vec3::NEG_Z);
    (offset, heading)
}

#[derive(Resource, Default)]
pub(crate) struct MotionReport {
    samples: Vec<Value>,
    last_sample: f64,
}

impl MotionReport {
    pub(crate) fn json(&self) -> Value {
        json!({
            "offscreen_start_s": PROBE_AWAY,
            "offscreen_end_s": PROBE_RETURN,
            "offscreen_completed": self.last_sample >= PROBE_RETURN,
            "blend_s": FADE.as_secs_f64(),
            "samples": self.samples,
        })
    }
}

#[derive(bevy::ecs::system::SystemParam)]
struct ProbeScene<'w, 's> {
    meshes: Query<'w, 's, &'static CreatureMesh>,
    camera: Query<'w, 's, &'static VisibleEntities, With<Camera3d>>,
    roots: Query<'w, 's, (Entity, &'static Actor, &'static Transform)>,
    players: Query<'w, 's, (Entity, &'static CreatureAnimation, &'static AnimationPlayer)>,
    joints: Query<
        'w,
        's,
        (
            &'static Name,
            &'static AnimatedBy,
            &'static Transform,
            &'static GlobalTransform,
        ),
    >,
}

fn probe_enabled(o: Res<Options>) -> bool {
    o.creature_probe || o.creature_record
}

fn record_probe(
    demo: Option<Res<CreatureDemo>>,
    assets: Option<Res<CreatureAssets>>,
    terrain: Res<Terrain>,
    scene: ProbeScene,
    mut report: ResMut<MotionReport>,
) {
    let (Some(demo), Some(assets)) = (demo, assets) else {
        return;
    };
    if !demo.ready || report.samples.len() >= 512 {
        return;
    }
    if !report.samples.is_empty() && demo.probe_elapsed - report.last_sample < 0.08 {
        return;
    }
    let mut visible = HashMap::<Entity, usize>::new();
    if let Ok(camera) = scene.camera.single() {
        for entity in camera.get(TypeId::of::<Mesh3d>()) {
            if let Ok(mesh) = scene.meshes.get(*entity) {
                *visible.entry(mesh.owner).or_default() += 1;
            }
        }
    }

    let mut actors = Vec::new();
    for (player_entity, binding, player) in &scene.players {
        let Ok((entity, actor, transform)) = scene.roots.get(binding.owner) else {
            continue;
        };
        let mut joints = serde_json::Map::new();
        if actor.slot == 0 {
            for (name, animated_by, transform, global) in &scene.joints {
                if animated_by.0 == player_entity
                    && matches!(
                        name.as_str(),
                        "Body"
                            | "Head"
                            | "Jaw"
                            | "WingWrist.L"
                            | "WingWrist.R"
                            | "Ankle.L"
                            | "TailTip"
                    )
                {
                    joints.insert(
                        name.to_string(),
                        json!({
                            "global": global.translation().to_array(),
                            "rotation": transform.rotation.to_array(),
                            "translation": transform.translation.to_array(),
                            "scale": transform.scale.to_array(),
                        }),
                    );
                }
            }
        }
        let clips: Vec<_> = CreatureClip::ALL
            .into_iter()
            .filter_map(|clip| {
                let active = player.animation(assets.node(clip)?)?;
                Some(json!({
                    "name": format!("{clip:?}"),
                    "weight": active.weight(),
                    "phase_s": active.seek_time(),
                    "completions": active.completions(),
                    "finished": active.is_finished(),
                }))
            })
            .collect();
        actors.push((
            actor.slot,
            json!({
                "slot": actor.slot,
                "state": format!("{:?}", actor.clip),
                "elapsed_s": actor.elapsed,
                "world": actor.position.to_array(),
                "local": transform.translation.to_array(),
                "forward": transform.forward().to_array(),
                "visible_meshes": visible.get(&entity).copied().unwrap_or(0),
                "clips": clips,
                "joints": joints,
            }),
        ));
    }
    actors.sort_by_key(|(slot, _)| *slot);
    report.samples.push(json!({
        "real_s": demo.probe_elapsed,
        "camera_away": demo.probe_yaw.is_some(),
        "playback_speed": demo.speed,
        "paused": demo.paused,
        "origin": terrain.origin,
        "actors": actors.into_iter().map(|(_, actor)| actor).collect::<Vec<_>>(),
    }));
    report.last_sample = demo.probe_elapsed;
}
#[cfg(test)]
mod tests {
    use super::*;

    const LENGTHS: [f64; 6] = [3.2, 1.04, 1.05, 1.02, 3.2, 1.1];

    #[test]
    fn sequence_visits_all_six_states_and_comes_to_rest() {
        let states = (0..=1800)
            .map(|frame| sequence_sample(frame as f64 / 100., LENGTHS))
            .fold(Vec::new(), |mut states, next| {
                if states.last() != Some(&next.clip) {
                    states.push(next.clip);
                }
                states
            });
        assert_eq!(
            states,
            [
                CreatureClip::IdleGround,
                CreatureClip::Walk,
                CreatureClip::Takeoff,
                CreatureClip::Flap,
                CreatureClip::Glide,
                CreatureClip::Land,
                CreatureClip::IdleGround,
            ]
        );
        let end = sequence_sample(30., LENGTHS);
        assert_eq!(end.offset.y, 0.);
        assert_eq!(end.offset, sequence_sample(60., LENGTHS).offset);
    }

    #[test]
    fn looping_and_one_shot_clip_phase_differ() {
        assert!((clip_phase(CreatureClip::Walk, 3.3, 1.) - 0.3).abs() < 1e-8);
        assert_eq!(clip_phase(CreatureClip::Takeoff, 3.3, 1.), 1.);
        assert_eq!(clip_phase(CreatureClip::Land, 3.3, 1.), 1.);
    }

    #[test]
    fn motion_uses_elapsed_time_and_position_is_continuous_at_switches() {
        let integrate = |hz| {
            let mut time = 0.;
            for _ in 0..8 * hz {
                time += 1. / f64::from(hz);
            }
            sequence_sample(time, LENGTHS)
        };
        let slow = integrate(24);
        let fast = integrate(165);
        assert_eq!(slow.clip, fast.clip);
        assert!((slow.offset - fast.offset).length() < 1e-10);
        assert!(slow.offset.y > 22.);
        let mut boundary = 0.;
        for stage in stage_lengths(LENGTHS) {
            boundary += stage;
            let before = sequence_sample(boundary - 1e-6, LENGTHS);
            let after = sequence_sample(boundary + 1e-6, LENGTHS);
            assert!((before.offset - after.offset).length() < 1e-4);
        }
    }

    #[test]
    fn rebasing_preserves_fractional_world_position() {
        let world = DVec3::new(-1_500_000_000.375, 25.5, 1_499_999_999.625);
        let first = crate::terrain::origin_for(world);
        let second = [first[0] - 1, first[1] + 1, first[2] + 1];
        for origin in [first, second] {
            assert_eq!(
                local_position(world, origin).as_dvec3() + origin_blocks(origin),
                world
            );
        }
    }
}
