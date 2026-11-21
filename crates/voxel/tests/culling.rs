use primal_voxel::{World, generate_chunk};

#[test]
fn cavity_and_exterior_are_front_facing_on_all_six_sides() {
    let chunk = [0, 7, 0];
    let mut world = World::streamed(19);
    let t = world.begin_load(chunk).unwrap();
    assert!(world.install_chunk(t, generate_chunk(19, chunk).unwrap(), &[]));
    for x in 5..8 {
        for y in 117..120 {
            for z in 5..8 {
                if [x, y, z] != [6, 118, 6] {
                    assert!(world.set([x, y, z], 3));
                }
            }
        }
    }
    let faces = world.mesh_snapshot(chunk).unwrap().greedy_mesh_local();
    let mut inward = 0;
    let mut outer = 0;
    for face in faces {
        let [a, b, c, d] = face.corners;
        let axis = face.normal.iter().position(|v| *v != 0).unwrap();
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        for (p, q) in [(b, c), (c, d)] {
            let signed = (p[u] - a[u]) * (q[v] - a[v]) - (p[v] - a[v]) * (q[u] - a[u]);
            assert!(signed * face.normal[axis] as f32 > 0.);
        }
        let center = (a[axis] + c[axis]) * 0.5;
        let observer = 6.5;
        if (observer - center) * face.normal[axis] as f32 > 0. {
            inward += 1;
        } else {
            outer += 1;
        }
    }
    assert_eq!((inward, outer), (6, 6));
}
