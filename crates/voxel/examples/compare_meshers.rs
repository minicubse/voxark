//! Release A/B on identical immutable chunk snapshots. Outputs every timed repetition.
use primal_voxel::{Face, MeshSnapshot, World};
use std::{hint::black_box, time::Instant};

#[derive(Clone, Copy)]
enum Method {
    Faces,
    Greedy,
    Binary,
    Reuse,
}

fn compare(label: &str, snapshots: &[MeshSnapshot]) {
    let mut scratch = Vec::<Face>::new();
    let methods = [Method::Faces, Method::Greedy, Method::Binary, Method::Reuse];
    for repeat in 0..12 {
        for offset in 0..methods.len() {
            let method = methods[(repeat + offset) % methods.len()];
            let t = Instant::now();
            let mut quads = 0;
            for snapshot in snapshots {
                quads += match method {
                    Method::Faces => black_box(snapshot.mesh()).len(),
                    Method::Greedy => black_box(snapshot.greedy_mesh()).len(),
                    Method::Binary => black_box(snapshot.binary_greedy_mesh()).len(),
                    Method::Reuse => {
                        snapshot.binary_greedy_into(&mut scratch);
                        black_box(&scratch).len()
                    }
                };
            }
            let elapsed = t.elapsed().as_nanos();
            let name = match method {
                Method::Faces => "faces",
                Method::Greedy => "greedy",
                Method::Binary => "binary",
                Method::Reuse => "binary_reuse",
            };
            println!(
                "{label},{repeat},{name},{},{quads},{elapsed}",
                snapshots.len()
            );
        }
    }
}

fn main() {
    println!("terrain,repeat,method,chunks,quads,ns");
    let mut world = World::new(42);
    let snapshots = world
        .chunks()
        .into_iter()
        .map(|c| world.mesh_snapshot(c).unwrap())
        .collect::<Vec<_>>();
    compare("valley", &snapshots);
    for (name, kind) in [
        ("empty", 0),
        ("solid", 1),
        ("checkerboard", 2),
        ("multi_material", 3),
    ] {
        for y in 32..48 {
            for z in 0..16 {
                for x in 0..16 {
                    let block = match kind {
                        0 => 0,
                        1 => 1,
                        2 => ((x + y + z) % 2) as u8,
                        _ => {
                            if x < 8 {
                                1 + ((y + z) % 7) as u8
                            } else {
                                0
                            }
                        }
                    };
                    world.set([x, y, z], block);
                }
            }
        }
        compare(name, &[world.mesh_snapshot([0, 2, 0]).unwrap()]);
    }
}
