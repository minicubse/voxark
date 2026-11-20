use primal_simulation::{Edit, FIXED_DT, Input, Player, edit, raycast};
use primal_voxel::{ChunkPos, World, generate_chunk};

fn load(world: &mut World, chunk: ChunkPos) {
    let ticket = world.begin_load(chunk).unwrap();
    assert!(world.install_chunk(ticket, generate_chunk(world.seed(), chunk).unwrap(), &[]));
}

#[test]
fn movement_stops_at_unknown_chunk_then_crosses_after_loading() {
    let mut world = World::streamed(42);
    load(&mut world, [-8, 7, 7]);
    let mut player = Player {
        position: [-112.7, 119.0, 113.5],
        velocity: [0.0; 3],
        grounded: false,
        flying: true,
    };
    let forward = Input {
        movement: [1.0, 0.0, 0.0],
        ..Input::default()
    };
    for _ in 0..15 {
        player.step(&world, forward, FIXED_DT);
    }
    assert!(player.position[0] <= -112.29);
    load(&mut world, [-7, 7, 7]);
    for _ in 0..15 {
        player.step(&world, forward, FIXED_DT);
    }
    assert!(player.position[0] > -111.0);
}

#[test]
fn ray_and_validated_edit_cannot_cross_an_unknown_chunk() {
    let mut world = World::streamed(42);
    load(&mut world, [200, 7, -100]);
    assert!(world.set([3200, 120, -1599], 7));
    let player = Player {
        position: [3198.5, 119.0, -1598.5],
        velocity: [0.0; 3],
        flying: true,
        grounded: false,
    };
    assert!(raycast(&world, player.eye(), [1.0, 0.0, 0.0], 6.0).is_none());
    assert!(edit(&mut world, &player, [1.0, 0.0, 0.0], Edit::Remove).is_err());
    load(&mut world, [199, 7, -100]);
    assert_eq!(
        edit(&mut world, &player, [1.0, 0.0, 0.0], Edit::Remove),
        Ok([3200, 120, -1599])
    );
}
