use std::hint::black_box;
use std::time::Instant;

use primal_voxel::{Face, World};

fn main() {
    const REPETITIONS: usize = 10;
    let mut generation = Vec::new();
    let mut meshing = Vec::new();
    let mut greedy = Vec::new();
    let mut baseline = Vec::new();
    let mut last_faces = 0;
    let mut greedy_faces = 0;
    let mut solid = 0;
    let mut prototype = World::new(42);
    prototype.set([0, 60, 0], 3);
    let mut cube = prototype.mesh([0, 3, 0]);
    assert_eq!(cube.len(), 6);
    for face in &mut cube {
        for corner in &mut face.corners {
            corner[1] -= 60.0;
        }
    }
    for _ in 0..REPETITIONS {
        let start = Instant::now();
        let world = black_box(World::new(42));
        generation.push(start.elapsed().as_secs_f64() * 1000.0);
        solid = world.solid_count();
        let start = Instant::now();
        last_faces = world
            .chunks()
            .into_iter()
            .map(|chunk| black_box(world.mesh(chunk)).len())
            .sum::<usize>();
        meshing.push(start.elapsed().as_secs_f64() * 1000.0);
        let start = Instant::now();
        greedy_faces = world
            .chunks()
            .into_iter()
            .map(|chunk| black_box(world.greedy_mesh(chunk)).len())
            .sum::<usize>();
        greedy.push(start.elapsed().as_secs_f64() * 1000.0);
        let start = Instant::now();
        assert_eq!(reference_mesh(&world, 16, false, &cube), solid * 6);
        baseline.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    generation.sort_by(f64::total_cmp);
    meshing.sort_by(f64::total_cmp);
    greedy.sort_by(f64::total_cmp);
    baseline.sort_by(f64::total_cmp);
    println!("seed=42 chunks=256 dimensions=128x64x128 samples={REPETITIONS}");
    println!(
        "generation_median_ms={:.3} generation_max_ms={:.3}",
        generation[REPETITIONS / 2],
        generation[REPETITIONS - 1]
    );
    println!(
        "culling_mesh_median_ms={:.3} culling_mesh_max_ms={:.3}",
        meshing[REPETITIONS / 2],
        meshing[REPETITIONS - 1]
    );
    println!(
        "greedy_mesh_median_ms={:.3} greedy_mesh_max_ms={:.3} greedy_quads={greedy_faces} reduction_vs_exposed_percent={:.2}",
        greedy[REPETITIONS / 2],
        greedy[REPETITIONS - 1],
        100.0 * (1.0 - greedy_faces as f64 / last_faces as f64)
    );
    println!(
        "full_cube_mesh_median_ms={:.3} full_cube_mesh_max_ms={:.3}",
        baseline[REPETITIONS / 2],
        baseline[REPETITIONS - 1]
    );
    println!(
        "solid_voxels={solid} full_cube_faces={} exposed_faces={last_faces} face_reduction_percent={:.2}",
        solid * 6,
        100.0 * (1.0 - last_faces as f64 / (solid * 6) as f64)
    );
    println!(
        "voxel_payload_bytes={} (current + original, excludes Vec metadata and sparse edits)",
        2 * 128 * 64 * 128
    );

    // Same sampler and face builder for all region sizes. This isolates traversal
    // and allocation effects, without claiming to compare storage layouts.
    let world = World::new(42);
    for size in [16, 32, 64] {
        let mut timings = Vec::new();
        for _ in 0..REPETITIONS {
            let start = Instant::now();
            assert_eq!(reference_mesh(&world, size, true, &cube), last_faces);
            timings.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        timings.sort_by(f64::total_cmp);
        println!(
            "reference_region_size={size} culling_median_ms={:.3} culling_max_ms={:.3}",
            timings[REPETITIONS / 2],
            timings[REPETITIONS - 1]
        );
    }
}

fn reference_mesh(world: &World, size: i32, cull: bool, cube: &[Face]) -> usize {
    let mut face_count = 0;
    for region_y in (0..64).step_by(size as usize) {
        for region_z in (-64..64).step_by(size as usize) {
            for region_x in (-64..64).step_by(size as usize) {
                let mut faces = Vec::new();
                for y in region_y..region_y + size {
                    for z in region_z..region_z + size {
                        for x in region_x..region_x + size {
                            let position = [x, y, z];
                            let block = world.get(position);
                            if block == 0 {
                                continue;
                            }
                            for prototype in cube {
                                let neighbour = std::array::from_fn(|axis| {
                                    position[axis] + prototype.normal[axis]
                                });
                                if cull && world.get(neighbour) != 0 {
                                    continue;
                                }
                                let mut face = *prototype;
                                face.block = block;
                                for corner in &mut face.corners {
                                    for axis in 0..3 {
                                        corner[axis] += position[axis] as f32;
                                    }
                                }
                                faces.push(face);
                            }
                        }
                    }
                }
                face_count += black_box(faces).len();
            }
        }
    }
    face_count
}
