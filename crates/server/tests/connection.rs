use primal_net::{link::Link, *};
use primal_voxel::{CHUNK, Preset, generate_for};
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
#[test]
fn client_receives_an_authoritative_spawn_chunk_over_udp() {
    let tmp = tempfile::tempdir().unwrap();
    let mut server = Command::new(env!("CARGO_BIN_EXE_primal-server"))
        .arg("--world")
        .arg(tmp.path().join("world.pvx"))
        .args(["--bind", "127.0.0.1:0", "--ticks", "150"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(server.stdout.as_mut().unwrap())
        .read_line(&mut line)
        .unwrap();
    let addr = serde_json::from_str::<serde_json::Value>(&line).unwrap()["address"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let mut c =
        ClientTransport::connect("127.0.0.1:0".parse().unwrap(), addr, ConnectionId(42)).unwrap();
    let mut hello = false;
    let mut rejected_versions = 0;
    let mut replica = None;
    let start = Instant::now();
    loop {
        c.poll(Duration::from_millis(10)).unwrap();
        if c.is_connected() && !hello {
            c.send(
                Channel::Control,
                encode(&ClientMsg::Hello {
                    version: if rejected_versions == 0 {
                        PROTOCOL_VERSION + 1
                    } else {
                        PROTOCOL_VERSION
                    },
                    generator: primal_voxel::GENERATOR_VERSION + u32::from(rejected_versions == 1),
                    token: [8; 32],
                })
                .unwrap(),
            )
            .unwrap();
            hello = true;
        }
        while let Some((_, b)) = c.recv() {
            let msg = decode(&b).unwrap();
            if let ServerMsg::Error(ref reason) = msg {
                assert!(reason.contains("Version incompatible"));
                rejected_versions += 1;
                assert!(rejected_versions <= 2);
                hello = false;
            }
            if let ServerMsg::Welcome { seed, .. } = msg {
                assert_eq!(rejected_versions, 2);
                replica = Some(Replica::new(seed));
                c.send(Channel::Control, encode(&ClientMsg::Ready).unwrap())
                    .unwrap();
            }
            if let Some(r) = replica.as_mut() {
                r.apply(&msg).unwrap();
            }
        }
        if replica.as_ref().is_some_and(|r| r.world.resident_len() > 0) {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        c.flush().unwrap();
        thread::sleep(Duration::from_millis(10));
    }
    c.disconnect();
    assert!(server.wait().unwrap().success());
}

#[test]
fn forest_welcome_and_full_chunks_replicate_over_protocol_v1() {
    let tmp = tempfile::tempdir().unwrap();
    let mut server = Command::new(env!("CARGO_BIN_EXE_primal-server"))
        .arg("--world")
        .arg(tmp.path().join("forest.pvx"))
        .args([
            "--preset",
            "forest-modern",
            "--bind",
            "127.0.0.1:0",
            "--ticks",
            "240",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut started = String::new();
    BufReader::new(server.stdout.as_mut().unwrap())
        .read_line(&mut started)
        .unwrap();
    let started: serde_json::Value = serde_json::from_str(&started).unwrap();
    let addr = started["address"].as_str().unwrap();
    let link = Link::open(addr, [10; 32], 60).unwrap();
    let mut welcome = None;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let msg = link
            .events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("le serveur doit répliquer la forêt");
        match msg {
            ServerMsg::Welcome { seed, .. } => {
                assert!(link.forest_modern());
                welcome = Some((seed, Replica::new(seed)));
            }
            ServerMsg::Chunk {
                chunk, ref data, ..
            } => {
                assert!(matches!(data, ChunkData::Full(_)));
                let (seed, replica) = welcome.as_mut().unwrap();
                replica.apply(&msg).unwrap();
                let forest = generate_for(Preset::ForestModern, *seed, chunk).unwrap();
                let origin = chunk.map(|axis| axis * CHUNK);
                let mut structure = false;
                for x in 0..CHUNK {
                    for y in 0..CHUNK {
                        for z in 0..CHUNK {
                            let position = [origin[0] + x, origin[1] + y, origin[2] + z];
                            let block = forest.get(position);
                            assert_eq!(replica.world.lookup(position), block);
                            structure |= matches!(block, Some(5 | 6));
                        }
                    }
                }
                if structure {
                    break;
                }
            }
            ServerMsg::Error(error) => panic!("{error}"),
            _ => {}
        }
    }
    drop(link);
    assert!(server.wait().unwrap().success());
}
