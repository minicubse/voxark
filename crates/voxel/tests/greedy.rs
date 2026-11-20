use std::collections::BTreeSet;

use primal_voxel::{Face, World};

type UnitFace = ([i32; 3], [i32; 3], u8);

fn unit_faces(faces: &[Face]) -> BTreeSet<UnitFace> {
    let mut units = BTreeSet::new();
    for face in faces {
        let minimum = std::array::from_fn::<_, 3, _>(|axis| {
            face.corners
                .iter()
                .map(|corner| corner[axis] as i32)
                .min()
                .unwrap()
        });
        let maximum = std::array::from_fn::<_, 3, _>(|axis| {
            face.corners
                .iter()
                .map(|corner| corner[axis] as i32)
                .max()
                .unwrap()
        });
        let normal_axis = face.normal.iter().position(|&value| value != 0).unwrap();
        let u = (normal_axis + 1) % 3;
        let v = (normal_axis + 2) % 3;
        assert_eq!(minimum[normal_axis], maximum[normal_axis]);
        assert!(maximum[u] > minimum[u] && maximum[v] > minimum[v]);
        let a = std::array::from_fn::<_, 3, _>(|i| face.corners[1][i] - face.corners[0][i]);
        let b = std::array::from_fn::<_, 3, _>(|i| face.corners[2][i] - face.corners[0][i]);
        let cross = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        assert!(cross[normal_axis] * face.normal[normal_axis] as f32 > 0.0);
        for cu in minimum[u]..maximum[u] {
            for cv in minimum[v]..maximum[v] {
                let mut position = minimum;
                position[u] = cu;
                position[v] = cv;
                assert!(
                    units.insert((position, face.normal, face.block)),
                    "duplicate face"
                );
            }
        }
    }
    units
}

#[test]
fn greedy_exactly_preserves_every_oriented_material_face_in_world() {
    let mut world = World::new(42);
    for edited in [false, true] {
        if edited {
            for position in [
                [-1, 31, 15],
                [0, 31, 15],
                [0, 32, 16],
                [-64, 0, -64],
                [63, 63, 63],
            ] {
                world.set(position, 7);
            }
            for position in [[-1, 10, 0], [0, 10, 0], [15, 15, 15], [16, 15, 15]] {
                world.set(position, 0);
            }
        }
        let mut reference_count = 0;
        let mut greedy_count = 0;
        for chunk in world.chunks() {
            let reference = world.mesh(chunk);
            let greedy = world.greedy_mesh(chunk);
            assert_eq!(
                unit_faces(&reference),
                unit_faces(&greedy),
                "chunk {chunk:?}, edited={edited}"
            );
            reference_count += reference.len();
            greedy_count += greedy.len();
        }
        assert!(greedy_count < reference_count / 2);
    }
    assert!(world.greedy_mesh([i32::MAX, 0, 0]).is_empty());
}

#[test]
fn binary_matches_exposed_faces_and_reused_buffer_clears_between_chunks() {
    let mut world = World::new(42);
    let mut output = Vec::new();
    for c in world.chunks() {
        let snapshot = world.mesh_snapshot(c).unwrap();
        snapshot.binary_greedy_into(&mut output);
        assert_eq!(
            unit_faces(&snapshot.mesh()),
            unit_faces(&output),
            "valley {c:?}"
        );
    }
    for case in 0..4 {
        for y in 32..48 {
            for z in 0..16 {
                for x in 0..16 {
                    let material = match case {
                        0 => 0,
                        1 => 3,
                        2 => {
                            if (x + y + z) % 2 == 0 {
                                7
                            } else {
                                0
                            }
                        }
                        _ => 1 + ((x + z + y) % 7) as u8,
                    };
                    world.set([x, y, z], material);
                }
            }
        }
        for c in [
            [0, 2, 0],
            [1, 2, 0],
            [-1, 2, 0],
            [0, 1, 0],
            [0, 3, 0],
            [0, 2, -1],
            [0, 2, 1],
        ] {
            let snapshot = world.mesh_snapshot(c).unwrap();
            snapshot.binary_greedy_into(&mut output);
            assert_eq!(
                unit_faces(&snapshot.mesh()),
                unit_faces(&output),
                "case {case} {c:?}"
            );
        }
    }
}
