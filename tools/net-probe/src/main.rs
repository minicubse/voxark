//! JSON-lines process probe. All world observations use the real replicated state.
use primal_net::{
    Actor, Channel, ClientMsg, ClientTransport, ConnectionId, PROTOCOL_VERSION, Replica, ServerMsg,
    decode, direction, encode, identity,
};
use primal_voxel::{GENERATOR_VERSION, Pos, chunk_at};
use serde_json::{Value, json};
use std::{
    env,
    io::{self, BufRead, Write},
    net::SocketAddr,
    path::Path,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn emit(value: Value) {
    println!("{value}");
    let _ = io::stdout().flush();
}
fn pos(v: &Value) -> Result<Pos, String> {
    serde_json::from_value(v.clone()).map_err(|e| e.to_string())
}
fn send(net: &mut ClientTransport, channel: Channel, msg: &ClientMsg) -> Result<(), String> {
    net.send(channel, encode(msg).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 3 {
        return Err("usage: primal-net-probe SERVER:PORT TOKEN_FILE".into());
    }
    let token = identity(Path::new(&args[2]))?;
    let id = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos() as u64
        ^ (u64::from(std::process::id()) << 32);
    let server: SocketAddr = args[1].parse()?;
    let mut net = ClientTransport::connect("127.0.0.1:0".parse()?, server, ConnectionId(id))?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            match line {
                Ok(line) => {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    let mut hello = false;
    let mut replica = None::<Replica>;
    let mut own = None;
    let mut players = Vec::<Actor>::new();
    let mut initial_actor = None::<Actor>;
    let mut seq = 0u64;
    let mut tick = 0;
    let mut chunks = 0u64;
    let mut movement = [0.0f32; 3];
    let mut yaw = 0.0f32;
    let mut pitch = 0.0f32;
    let mut flying = false;
    let mut previous = Instant::now();
    let mut next_input = Instant::now();
    loop {
        let now = Instant::now();
        net.poll(now.duration_since(previous))?;
        previous = now;
        if net.is_connected() && !hello {
            send(
                &mut net,
                Channel::Control,
                &ClientMsg::Hello {
                    version: PROTOCOL_VERSION,
                    generator: GENERATOR_VERSION,
                    token,
                },
            )?;
            hello = true;
        }
        while let Some((_, bytes)) = net.recv() {
            let msg: ServerMsg = decode(&bytes)?;
            match &msg {
                ServerMsg::Welcome { id, seed } => {
                    own = Some(*id);
                    replica = Some(Replica::new(*seed));
                    send(&mut net, Channel::Control, &ClientMsg::Ready)?;
                    emit(json!({"event":"welcome","id":id,"seed":seed}));
                }
                ServerMsg::Players {
                    tick: t,
                    players: p,
                } => {
                    if initial_actor.is_none()
                        && let Some(actor) = p.iter().find(|actor| Some(actor.id) == own)
                    {
                        yaw = actor.yaw;
                        pitch = actor.pitch;
                        flying = actor.flying;
                        initial_actor = Some(actor.clone());
                    }
                    if *t >= tick {
                        tick = *t;
                        players = p.clone();
                    }
                }
                ServerMsg::Chunk { .. } => {
                    chunks += 1;
                }
                ServerMsg::Outcome {
                    seq,
                    accepted,
                    reason,
                } => emit(json!({"event":"outcome","seq":seq,"accepted":accepted,"reason":reason})),
                ServerMsg::Error(e) => emit(json!({"event":"server_error","message":e})),
                _ => (),
            }
            if let Some(r) = &mut replica {
                r.apply(&msg)?;
            }
        }
        while let Ok(line) = rx.try_recv() {
            let request: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    emit(json!({"event":"bad_request","error":e.to_string()}));
                    continue;
                }
            };
            let result = (|| -> Result<Value, String> {
                match request["op"].as_str().unwrap_or("") {
                    "status" => Ok(
                        json!({"id":own,"initial_actor":initial_actor,"players":players,"tick":tick,"chunks_received":chunks,"connected":net.is_connected(),"rtt_ms":net.rtt().as_secs_f64()*1000.0,"sent_bytes":net.stats().sent_bytes,"received_bytes":net.stats().received_bytes}),
                    ),
                    "block" => {
                        let p = pos(&request["pos"])?;
                        let r = replica.as_ref().ok_or("no welcome")?;
                        Ok(
                            json!({"pos":p,"material":r.world.lookup(p),"revision":r.revision(chunk_at(p))}),
                        )
                    }
                    "surface" => {
                        let x = request["x"].as_i64().ok_or("x absent")? as i32;
                        let z = request["z"].as_i64().ok_or("z absent")? as i32;
                        let r = replica.as_ref().ok_or("no welcome")?;
                        for y in (0..128).rev() {
                            let p = [x, y, z];
                            match r.world.lookup(p) {
                                Some(0) => (),
                                Some(b) => return Ok(json!({"pos":p,"material":b})),
                                None => return Err("column incomplete".into()),
                            }
                        }
                        Err("empty column".into())
                    }
                    "input" => {
                        if request.get("movement").is_some() {
                            movement = serde_json::from_value(request["movement"].clone())
                                .map_err(|e| e.to_string())?;
                        }
                        if let Some(x) = request["yaw"].as_f64() {
                            yaw = x as f32;
                        }
                        if let Some(x) = request["pitch"].as_f64() {
                            pitch = x as f32;
                        }
                        if let Some(x) = request["flying"].as_bool() {
                            flying = x;
                        }
                        Ok(json!({"ok":true}))
                    }
                    "ray" => {
                        let r = replica.as_ref().ok_or("no welcome")?;
                        let p = players
                            .iter()
                            .find(|p| Some(p.id) == own)
                            .ok_or("no player snapshot")?;
                        let eye = [p.position[0], p.position[1] + 1.62, p.position[2]];
                        let hit = primal_simulation::raycast(
                            &r.world,
                            eye,
                            direction(yaw, pitch),
                            primal_simulation::MAX_REACH,
                        )
                        .ok_or("ray misses")?;
                        Ok(
                            json!({"block":hit.block,"adjacent":hit.adjacent,"material":r.world.lookup(hit.block),"block_revision":r.revision(chunk_at(hit.block)),"adjacent_revision":r.revision(chunk_at(hit.adjacent)),"distance":hit.distance,"yaw":yaw,"pitch":pitch}),
                        )
                    }
                    "edit" => {
                        let p = pos(&request["pos"])?;
                        let revision = match request["revision"].as_u64() {
                            Some(v) => v,
                            None => replica
                                .as_ref()
                                .and_then(|r| r.revision(chunk_at(p)))
                                .ok_or("chunk absent")?,
                        };
                        let material = request["material"].as_u64().ok_or("material absent")? as u8;
                        seq += 1;
                        send(
                            &mut net,
                            Channel::Edits,
                            &ClientMsg::Edit {
                                seq,
                                target: p,
                                revision,
                                material,
                                yaw,
                                pitch,
                            },
                        )?;
                        Ok(json!({"seq":seq,"revision":revision}))
                    }
                    "quit" => {
                        send(&mut net, Channel::Control, &ClientMsg::Bye)?;
                        net.flush().map_err(|e| e.to_string())?;
                        Ok(json!({"bye":true}))
                    }
                    _ => Err("unknown operation".into()),
                }
            })();
            let quitting = request["op"] == "quit" && result.is_ok();
            match result {
                Ok(result) => emit(json!({"request_id":request["request_id"],"result":result})),
                Err(error) => emit(json!({"request_id":request["request_id"],"error":error})),
            }
            if quitting {
                return Ok(());
            }
        }
        if initial_actor.is_some() && now >= next_input {
            seq += 1;
            send(
                &mut net,
                Channel::Input,
                &ClientMsg::Input {
                    seq,
                    movement,
                    jump: false,
                    sprint: false,
                    flying,
                    yaw,
                    pitch,
                },
            )?;
            next_input = now + Duration::from_millis(16);
        }
        net.flush()?;
        if let Some(reason) = net.disconnect_reason() {
            return Err(reason.into());
        }
        thread::sleep(Duration::from_millis(4));
    }
}
fn main() {
    if let Err(e) = run() {
        emit(json!({"event":"fatal","error":e.to_string()}));
        std::process::exit(1);
    }
}
