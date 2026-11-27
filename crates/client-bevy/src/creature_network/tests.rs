use super::*;
use primal_simulation::creatures::{CreatureId, Species};
use std::time::Duration;

fn wyvern(id: u64, x: f64, yaw: f32, locomotion: Locomotion, start: u64) -> CreatureState {
    CreatureState {
        id: CreatureId(id),
        species: Species::Wyvern,
        palette: Palette::Jade,
        position: [x, 2., -1.],
        yaw,
        locomotion,
        state_start_tick: start,
    }
}

#[test]
fn ignores_duplicate_and_inverted_frames_and_bounds_holes() {
    let now = Instant::now();
    let mut net = NetworkCreatures::default();
    let a = wyvern(1, 0., 3.1, Locomotion::Walk, 1);
    let b = wyvern(1, 3., -3.1, Locomotion::Takeoff, 12);
    assert!(net.receive(10, 20, vec![a], now));
    assert!(net.receive(13, 20, vec![b], now));
    assert!(!net.receive(12, 20, vec![], now));
    assert!(!net.receive(13, 20, vec![], now));
    let track = &net.tracks[&1];
    let sample = track.sample(11.5, 20);
    assert_eq!(sample.position.x, 1.5);
    assert!((sample.yaw.abs() - std::f32::consts::PI).abs() < 0.000_01);
    assert_eq!(sample.current.clip, CreatureClip::Walk);
    let sample = track.sample(12.5, 20);
    assert_eq!(sample.current.clip, CreatureClip::Takeoff);
    assert_eq!(sample.current.elapsed, 0.025);
    assert_eq!(sample.previous.unwrap().clip, CreatureClip::Walk);
    assert!(net.receive(40, 20, vec![wyvern(1, 40., 0., Locomotion::Glide, 30)], now));
    assert_eq!(net.tracks[&1].sample(30., 20).position.x, 3.);
    assert_eq!(net.tracks[&1].sample(40., 20).position.x, 40.);
    assert!(net.tracks[&1].sample(40., 20).previous.is_none());
    assert_eq!(net.advance(now + Duration::from_secs(3)), Some(40.));
    assert_eq!(net.advance(now + Duration::from_secs(4)), Some(40.));
}

#[test]
fn late_join_exit_return_and_reconnect_recover_authoritative_phase() {
    let now = Instant::now();
    let mut net = NetworkCreatures::default();
    let state = || wyvern(7, 4., 0.25, Locomotion::Flap, 100);
    net.receive(130, 20, vec![state(), state()], now);
    assert_eq!(net.tracks.len(), 1);
    let sample = net.tracks[&7].sample(128., 20);
    assert_eq!(sample.tick, 130.);
    assert_eq!(sample.current.elapsed, 1.5);
    assert!(sample.previous.is_none());
    net.receive(132, 20, vec![], now);
    assert!(net.tracks.is_empty());
    net.receive(135, 20, vec![state()], now);
    assert_eq!(net.tracks[&7].entered_tick, 135);
    assert_eq!(net.tracks[&7].sample(133., 20).current.elapsed, 1.75);
    net = NetworkCreatures::default();
    assert!(net.receive(
        12,
        20,
        vec![wyvern(7, 0., 0., Locomotion::IdleGround, 1)],
        now
    ));
    assert_eq!(net.advance(now), Some(12.));
    assert_eq!(net.tracks[&7].sample(12., 20).current.elapsed, 0.55);
}

#[test]
fn floating_origin_preserves_fractional_server_coordinates() {
    let position = DVec3::new(-1_500_000_000.375, 32.25, 1_499_999_999.625);
    let origin = [-93_750_000, 2, 93_750_000];
    assert_eq!(
        local_position(position, origin),
        Vec3::new(-0.375, 0.25, -0.375)
    );
    assert_eq!(
        local_position(position, [-93_750_001, 2, 93_750_001]),
        Vec3::new(15.625, 0.25, -16.375)
    );
}
