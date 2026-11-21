use primal_net::*;
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
