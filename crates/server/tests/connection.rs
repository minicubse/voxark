use primal_net::{link::Link, *};
use primal_simulation::creatures::{CreatureState, CreatureWorld};
use primal_voxel::{CHUNK, Preset, generate_for};
use std::{
    io::{BufRead, BufReader, Write},
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
                        CREATURE_PROTOCOL_VERSION + 1
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

#[test]
fn opted_in_client_gets_an_authoritative_creature_snapshot() {
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
    let mut client =
        ClientTransport::connect("127.0.0.1:0".parse().unwrap(), addr, ConnectionId(47)).unwrap();
    let dt = Duration::from_millis(10);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut hello = false;
    'snapshots: loop {
        client.poll(dt).unwrap();
        if client.is_connected() && !hello {
            client
                .send(
                    Channel::Control,
                    encode(&ClientMsg::Hello {
                        version: 2,
                        generator: primal_voxel::GENERATOR_VERSION,
                        token: [11; 32],
                    })
                    .unwrap(),
                )
                .unwrap();
            hello = true;
        }
        while let Some((channel, bytes)) = client.recv() {
            if channel == Channel::Snapshots && bytes.first() == Some(&6) {
                break 'snapshots;
            }
            match decode::<ServerMsg>(&bytes).unwrap() {
                ServerMsg::Welcome { .. } => client
                    .send(Channel::Control, encode(&ClientMsg::Ready).unwrap())
                    .unwrap(),
                ServerMsg::Error(reason) => panic!("creature protocol rejected: {reason}"),
                _ => {}
            }
        }
        assert!(Instant::now() < deadline, "creature snapshot missing");
        client.flush().unwrap();
        thread::sleep(dt);
    }
    client.disconnect();
    assert!(server.wait().unwrap().success());
}

fn receive_until<T>(link: &Link, mut select: impl FnMut(ServerMsg) -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let message = link
            .events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("timed out waiting for server state");
        if let ServerMsg::Error(reason) = &message {
            panic!("server rejected the connection: {reason}");
        }
        if let Some(value) = select(message) {
            return value;
        }
    }
}

fn player_id(link: &Link) -> (u64, u64) {
    receive_until(link, |message| match message {
        ServerMsg::Welcome { id, seed } => Some((id, seed)),
        _ => None,
    })
}

fn creatures(link: &Link, minimum_tick: u64, visible: bool) -> (u64, Vec<CreatureState>) {
    receive_until(link, |message| match message {
        ServerMsg::Creatures {
            tick,
            tick_hz,
            creatures,
        } if tick >= minimum_tick && (visible != creatures.is_empty()) => {
            assert_eq!(tick_hz, 60);
            assert!(tick.is_multiple_of(3));
            Some((tick, creatures))
        }
        _ => None,
    })
}

#[test]
fn late_join_and_reconnect_resample_creatures_without_sending_them_to_v1() {
    let tmp = tempfile::tempdir().unwrap();
    let mut server = Command::new(env!("CARGO_BIN_EXE_primal-server"))
        .arg("--world")
        .arg(tmp.path().join("world.pvx"))
        .args([
            "--bind",
            "127.0.0.1:0",
            "--ticks",
            "720",
            "--creatures",
            "1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut started = String::new();
    BufReader::new(server.stdout.as_mut().unwrap())
        .read_line(&mut started)
        .unwrap();
    let started: serde_json::Value = serde_json::from_str(&started).unwrap();
    let addr = started["address"].as_str().unwrap();
    let legacy = Link::open(addr, [15; 32], 60).unwrap();
    receive_until(&legacy, |message| match message {
        ServerMsg::Chunk { .. } => Some(()),
        ServerMsg::Creatures { .. } => panic!("v1 received a creature snapshot"),
        _ => None,
    });

    let first = Link::open_creatures(addr, [16; 32], 60).unwrap();
    let (_, seed) = player_id(&first);
    let expected = CreatureWorld::new(seed, Preset::Historical, 1, 60);
    let (first_tick, first_state) = creatures(&first, 0, true);
    assert_eq!(first_state, expected.states(first_tick).collect::<Vec<_>>());

    let second_token = [17; 32];
    let second = Link::open_creatures(addr, second_token, 60).unwrap();
    let (second_id, _) = player_id(&second);
    let (joined_tick, joined_state) = creatures(&second, first_tick + 1, true);
    assert_eq!(
        joined_state,
        expected.states(joined_tick).collect::<Vec<_>>(),
    );
    let (same_tick, same_state) = creatures(&first, joined_tick, true);
    assert_eq!((same_tick, same_state), (joined_tick, joined_state));

    let stdin = server.stdin.as_mut().unwrap();
    writeln!(stdin, "teleport {second_id} 180 80 180").unwrap();
    let (outside_tick, outside) = creatures(&second, joined_tick + 1, false);
    assert!(outside.is_empty());
    writeln!(stdin, "teleport {second_id} 0 80 0").unwrap();
    let (return_tick, returned) = creatures(&second, outside_tick + 1, true);
    assert_eq!(returned[0].id, first_state[0].id);
    assert_eq!(returned, expected.states(return_tick).collect::<Vec<_>>());

    drop(second);
    receive_until(&first, |message| match message {
        ServerMsg::Players { tick, players }
            if tick > return_tick && players.iter().all(|p| p.id != second_id) =>
        {
            Some(())
        }
        _ => None,
    });
    let reconnected = Link::open_creatures(addr, second_token, 60).unwrap();
    assert_eq!(player_id(&reconnected), (second_id, seed));
    let (reconnected_tick, reconnected_state) = creatures(&reconnected, return_tick + 1, true);
    assert_eq!(
        reconnected_state,
        expected.states(reconnected_tick).collect::<Vec<_>>(),
    );
    let traffic = reconnected.creature_traffic();
    assert!(traffic.bytes > 0 && traffic.messages > 0);
    assert_eq!(legacy.creature_traffic().messages, 0);
    drop(reconnected);
    drop(first);
    drop(legacy);
    writeln!(stdin, "stop").unwrap();
    assert!(server.wait().unwrap().success());
}
