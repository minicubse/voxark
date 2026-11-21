//! Graphical replica: terrain and player positions originate only from the server.
use crate::{
    buffers::Buffers,
    metrics::FrameMetrics,
    options::Options,
    terrain::{self, Terrain},
};
use macroquad::prelude::*;
use primal_net::{
    Actor, Channel, ClientMsg, ClientTransport, ConnectionId, PROTOCOL_VERSION, Replica, ServerMsg,
};
use primal_simulation::{MAX_REACH, raycast};
use primal_voxel::{ChunkPos, GENERATOR_VERSION, MeshSnapshot, chunk_at};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::{SocketAddr, ToSocketAddrs},
    sync::mpsc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Remote {
    from: [f32; 3],
    actor: Actor,
    updated: f64,
}
impl Remote {
    fn position(&self, now: f64) -> [f32; 3] {
        interpolate(
            self.from,
            self.actor.position,
            ((now - self.updated) / 0.10).clamp(0., 1.) as f32,
        )
    }
}
fn interpolate(from: [f32; 3], to: [f32; 3], amount: f32) -> [f32; 3] {
    std::array::from_fn(|i| from[i] + (to[i] - from[i]) * amount)
}

struct Mesher {
    jobs: mpsc::SyncSender<MeshSnapshot>,
    results: mpsc::Receiver<(ChunkPos, u64, Buffers)>,
    inflight: BTreeSet<ChunkPos>,
    dirty: BTreeSet<ChunkPos>,
}
impl Mesher {
    fn new() -> Self {
        let (jobs, receive) = mpsc::sync_channel::<MeshSnapshot>(4);
        let (send, results) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(snapshot) = receive.recv() {
                let result = (
                    snapshot.chunk(),
                    snapshot.revision(),
                    Buffers::prepare(&snapshot.greedy_mesh()).0,
                );
                if send.send(result).is_err() {
                    break;
                }
            }
        });
        Self {
            jobs,
            results,
            inflight: BTreeSet::new(),
            dirty: BTreeSet::new(),
        }
    }
    fn tick(&mut self, replica: &Replica, terrain: &mut Terrain, position: [f32; 3]) {
        for _ in 0..4 {
            let Ok((chunk, revision, buffers)) = self.results.try_recv() else {
                break;
            };
            self.inflight.remove(&chunk);
            if replica.world.mesh_revision(chunk) == Some(revision) {
                terrain.install(chunk, buffers);
            } else if replica.world.is_resident(chunk) {
                self.dirty.insert(chunk);
            }
        }
        let center = chunk_at(position.map(|v| v.floor() as i32));
        let mut pending: Vec<_> = self.dirty.iter().copied().collect();
        pending.sort_by_key(|c| (c[0] - center[0]).abs() + (c[2] - center[2]).abs());
        for chunk in pending {
            if !replica.world.is_resident(chunk) {
                terrain.remove(chunk);
                self.dirty.remove(&chunk);
                continue;
            }
            if self.inflight.contains(&chunk) {
                continue;
            }
            let Some(snapshot) = replica.world.mesh_snapshot(chunk) else {
                continue;
            };
            match self.jobs.try_send(snapshot) {
                Ok(()) => {
                    self.inflight.insert(chunk);
                    self.dirty.remove(&chunk);
                }
                Err(mpsc::TrySendError::Full(_)) => break,
                Err(mpsc::TrySendError::Disconnected(_)) => break,
            }
        }
    }
}

fn send(transport: &mut ClientTransport, channel: Channel, msg: &ClientMsg) -> Result<(), String> {
    transport
        .send(channel, primal_net::encode(msg).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

pub async fn run(options: &Options, address: &str) -> Result<(), String> {
    let server = address
        .to_socket_addrs()
        .map_err(|e| format!("Adresse invalide : {e}"))?
        .next()
        .ok_or("Adresse sans IP")?;
    let bind: SocketAddr = if server.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    }
    .parse()
    .unwrap();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos() as u64
        ^ u64::from(std::process::id());
    let mut transport = ClientTransport::connect(
        bind,
        server,
        ConnectionId::random().map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let token = primal_net::identity(&options.identity).map_err(|e| format!("Identité : {e}"))?;
    let result = session(options, address, &mut transport, nonce, token).await;
    let _ = send(&mut transport, Channel::Control, &ClientMsg::Bye);
    let _ = transport.flush();
    if transport.is_connected() {
        let leaving = Instant::now();
        while leaving.elapsed() < Duration::from_millis(180) {
            let _ = transport.poll(Duration::from_millis(16));
            let _ = transport.flush();
            next_frame().await;
        }
        transport.disconnect();
    }
    set_cursor_grab(false);
    show_mouse(true);
    result
}

async fn session(
    options: &Options,
    address: &str,
    transport: &mut ClientTransport,
    mut seq: u64,
    token: [u8; 32],
) -> Result<(), String> {
    let start = Instant::now();
    let mut last_poll = Instant::now();
    let mut replica = None;
    let mut own_id = None;
    let mut hello_sent = false;
    let mut own = None;
    let mut displayed_position: Option<Vec3> = None;
    let mut remotes: BTreeMap<u64, Remote> = BTreeMap::new();
    let mut terrain = Terrain::default();
    let mut mesher = Mesher::new();
    let mut yaw: f32 = -0.35;
    let mut pitch: f32 = -0.12;
    let mut captured = false;
    let mut flying = false;
    let mut selected = 1_u8;
    let mut input_clock = 0.;
    let mut jump = false;
    let mut frame = 0;
    let mut metrics = FrameMetrics::default();
    let mut status = "Connexion…".to_owned();
    let mut max_peers = 0;
    let mut max_visible_peers = 0;
    let mut changed_peer_poses = 0;
    let mut peer_ids = BTreeSet::new();
    let mut snapshots = 0;
    let mut last_snapshot_tick = None;
    let mut accepted_edits = 0;
    let mut rejected_edits = 0;
    let mut maximum_quads = 0;
    let mut maximum_chunks = 0;
    let mut rtt_samples = Vec::new();
    let mut ready_at = None;
    let mut captured_peer = false;
    let fog = terrain::fog_material().ok();
    loop {
        let now = Instant::now();
        transport
            .poll(now.duration_since(last_poll))
            .map_err(|e| e.to_string())?;
        last_poll = now;
        if transport.is_connected() && !hello_sent {
            send(
                transport,
                Channel::Control,
                &ClientMsg::Hello {
                    version: PROTOCOL_VERSION,
                    generator: GENERATOR_VERSION,
                    token,
                },
            )?;
            hello_sent = true;
        }
        while let Some((_channel, bytes)) = transport.recv() {
            let msg: ServerMsg = primal_net::decode(&bytes)
                .map_err(|e| format!("Réponse serveur invalide : {e}"))?;
            match &msg {
                ServerMsg::Welcome { id, seed } => {
                    own_id = Some(*id);
                    replica = Some(Replica::new(*seed));
                    status = "Connecté · monde autoritaire".into();
                    send(transport, Channel::Control, &ClientMsg::Ready)?;
                }
                ServerMsg::Players { tick, players } => {
                    if own_id.is_none() || last_snapshot_tick.is_some_and(|last| *tick <= last) {
                        continue;
                    }
                    last_snapshot_tick = Some(*tick);
                    snapshots += 1;
                    let mut present = BTreeSet::new();
                    for actor in players {
                        if Some(actor.id) == own_id {
                            if own.is_none() {
                                flying = actor.flying;
                                yaw = actor.yaw;
                                pitch = actor.pitch;
                            }
                            own = Some(actor.clone());
                        } else {
                            present.insert(actor.id);
                            if options.net_report.is_some() {
                                peer_ids.insert(actor.id);
                            }
                            if let Some(old) = remotes.get(&actor.id)
                                && old.actor.position != actor.position
                            {
                                changed_peer_poses += 1;
                            }
                            let from = remotes
                                .get(&actor.id)
                                .map_or(actor.position, |r| r.position(get_time()));
                            remotes.insert(
                                actor.id,
                                Remote {
                                    from,
                                    actor: actor.clone(),
                                    updated: get_time(),
                                },
                            );
                        }
                    }
                    remotes.retain(|id, _| present.contains(id));
                    max_peers = max_peers.max(remotes.len());
                }
                ServerMsg::Outcome {
                    accepted, reason, ..
                } => {
                    if *accepted {
                        accepted_edits += 1;
                    } else {
                        rejected_edits += 1;
                    }
                    status = if *accepted {
                        "Modification confirmée par le serveur".into()
                    } else {
                        format!("Refus serveur : {reason}")
                    };
                }
                ServerMsg::Error(error) => return Err(format!("Serveur : {error}")),
                _ => {}
            }
            if let Some(replica) = replica.as_mut() {
                mesher
                    .dirty
                    .extend(replica.apply(&msg).map_err(|e| e.to_string())?);
            }
        }
        if !transport.is_connected()
            && (own_id.is_some() || transport.disconnect_reason().is_some())
        {
            return Err(format!(
                "Déconnecté : {}",
                transport
                    .disconnect_reason()
                    .unwrap_or_else(|| "serveur indisponible".into())
            ));
        }
        if own.is_none() && start.elapsed() > Duration::from_secs(20) {
            return Err("Le serveur n'a pas envoyé de joueur sous 20 s".into());
        }
        let dt = get_frame_time().min(0.1);
        if is_quit_requested() {
            break;
        }
        if is_key_pressed(KeyCode::Escape) {
            if !captured {
                break;
            }
            captured = false;
            set_cursor_grab(false);
            show_mouse(true);
        }
        let just_captured = !captured && is_mouse_button_pressed(MouseButton::Left);
        if just_captured {
            captured = true;
            set_cursor_grab(true);
            show_mouse(false);
        }
        if captured && !just_captured {
            let delta = mouse_delta_position();
            yaw += delta.x * 2.5;
            pitch = (pitch + delta.y * 2.).clamp(-1.52, 1.52);
        }
        if is_key_pressed(KeyCode::F) {
            flying = !flying;
        }
        for (i, key) in [
            KeyCode::Key1,
            KeyCode::Key2,
            KeyCode::Key3,
            KeyCode::Key4,
            KeyCode::Key5,
            KeyCode::Key6,
            KeyCode::Key7,
        ]
        .iter()
        .enumerate()
        {
            if is_key_pressed(*key) {
                selected = i as u8 + 1;
            }
        }
        let wheel = mouse_wheel().1;
        if wheel != 0. {
            selected = ((i32::from(selected) - 1 - wheel.signum() as i32).rem_euclid(7) + 1) as u8;
        }
        let benchmark = options.net_frames.is_some();
        let forward = vec3(yaw.sin(), 0., yaw.cos());
        let right = forward.cross(Vec3::Y);
        let mut movement = Vec3::ZERO;
        if captured {
            movement = forward
                * crate::axis(
                    is_key_down(KeyCode::W) || is_key_down(KeyCode::Z),
                    is_key_down(KeyCode::S),
                )
                + right
                    * crate::axis(
                        is_key_down(KeyCode::D),
                        is_key_down(KeyCode::A) || is_key_down(KeyCode::Q),
                    )
                + Vec3::Y
                    * crate::axis(
                        is_key_down(KeyCode::Space),
                        is_key_down(KeyCode::LeftControl),
                    );
            jump |= is_key_pressed(KeyCode::Space);
        }
        if benchmark {
            flying = true;
            let t = start.elapsed().as_secs_f32();
            let side = if own_id.unwrap_or(0) % 2 == 0 {
                1.
            } else {
                -1.
            };
            movement = vec3(
                (t * 0.3).sin() * side * 0.25,
                0.,
                (t * 0.3).cos() * side * 0.25,
            );
            yaw = t * 0.13;
            pitch = -0.23;
        }
        input_clock += dt;
        if own.is_some() && input_clock >= 1. / 60. {
            input_clock %= 1. / 60.;
            seq = seq.wrapping_add(1);
            send(
                transport,
                Channel::Input,
                &ClientMsg::Input {
                    seq,
                    movement: movement.to_array(),
                    jump,
                    sprint: captured && is_key_down(KeyCode::LeftShift),
                    flying,
                    yaw,
                    pitch,
                },
            )?;
            jump = false;
        }
        let front = Vec3::from_array(primal_net::direction(yaw, pitch));
        if let (Some(replica), Some(actor)) = (&replica, &own) {
            let eye = Vec3::from_array(actor.position) + Vec3::Y * 1.62;
            let remove = captured && !just_captured && is_mouse_button_pressed(MouseButton::Left);
            let place = captured && is_mouse_button_pressed(MouseButton::Right);
            if (remove || place)
                && let Some(hit) =
                    raycast(&replica.world, eye.to_array(), front.to_array(), MAX_REACH)
            {
                let target = if remove { hit.block } else { hit.adjacent };
                if let Some(revision) = replica.revision(chunk_at(target)) {
                    seq = seq.wrapping_add(1);
                    send(
                        transport,
                        Channel::Edits,
                        &ClientMsg::Edit {
                            seq,
                            target,
                            revision,
                            material: if remove { 0 } else { selected },
                            yaw,
                            pitch,
                        },
                    )?;
                }
            }
            mesher.tick(replica, &mut terrain, actor.position);
        }
        crate::draw_sky();
        let mut visible_peers = 0;
        if let Some(actor) = &own {
            let authoritative = Vec3::from_array(actor.position);
            let display = displayed_position.get_or_insert(authoritative);
            if display.distance_squared(authoritative) > 16.0 {
                *display = authoritative;
            } else {
                *display = display.lerp(authoritative, 1.0 - (-24.0 * dt).exp());
            }
            let eye = *display + Vec3::Y * 1.62;
            let distance = (options.stream.render_radius as f32 * 16.).max(32.);
            set_camera(&Camera3D {
                position: eye,
                target: eye + front,
                up: Vec3::Y,
                fovy: terrain::VERTICAL_FOV,
                z_near: 0.05,
                z_far: distance + 48.,
                ..Default::default()
            });
            if let Some(fog) = &fog {
                fog.set_uniform("CameraPosition", eye);
                fog.set_uniform("FogDistance", distance);
                gl_use_material(fog);
            }
            terrain.draw(eye, front, distance, options.frustum);
            gl_use_default_material();
            for remote in remotes.values() {
                let feet = Vec3::from_array(remote.position(get_time()));
                if (feet - eye).length() > distance || (feet - eye).dot(front) < -1. {
                    continue;
                }
                visible_peers += 1;
                let body = feet + Vec3::Y * 0.8;
                draw_cube(
                    body,
                    vec3(0.6, 1.6, 0.6),
                    None,
                    Color::from_rgba(201, 135, 64, 255),
                );
                draw_cube(
                    feet + Vec3::Y * 1.65,
                    vec3(0.45, 0.45, 0.45),
                    None,
                    Color::from_rgba(236, 203, 157, 255),
                );
                let look =
                    Vec3::from_array(primal_net::direction(remote.actor.yaw, remote.actor.pitch));
                draw_line_3d(feet + Vec3::Y * 1.65, feet + Vec3::Y * 1.65 + look, GOLD);
            }
            set_default_camera();
            if ready_at.is_none() && terrain.len() > 0 {
                ready_at = Some(start.elapsed().as_secs_f64() * 1000.);
            }
        }
        maximum_quads = maximum_quads.max(terrain.faces);
        maximum_chunks = maximum_chunks.max(terrain.len());
        max_visible_peers = max_visible_peers.max(visible_peers);
        let rtt = transport.rtt().as_secs_f64() * 1000.;
        if options.net_report.is_some() && own.is_some() {
            rtt_samples.push(rtt);
        }
        draw_rectangle(
            16.,
            16.,
            screen_width() - 32.,
            104.,
            Color::from_rgba(15, 31, 28, 220),
        );
        draw_text("PRIMAL VOXEL · MULTIJOUEUR", 30., 45., 26., WHITE);
        draw_text(
            format!(
                "{address} · RTT {rtt:.0} ms · {} joueurs · {} FPS · {} chunks",
                remotes.len() + usize::from(own.is_some()),
                get_fps(),
                terrain.len()
            ),
            30.,
            72.,
            18.,
            LIGHTGRAY,
        );
        draw_text(
            &status,
            30.,
            99.,
            18.,
            if rejected_edits > 0 {
                ORANGE
            } else {
                LIGHTGRAY
            },
        );
        draw_text(
            format!(
                "ZQSD/WASD · espace · maj · F vol · blocs 1–7 · clics éditer ({selected}) · Échap libère / déconnecte"
            ),
            20.,
            screen_height() - 22.,
            18.,
            WHITE,
        );
        let center = vec2(screen_width() * 0.5, screen_height() * 0.5);
        draw_line(center.x - 6., center.y, center.x + 6., center.y, 1., WHITE);
        draw_line(center.x, center.y - 6., center.x, center.y + 6., 1., WHITE);
        if !captured && !benchmark {
            draw_text(
                "Cliquez pour explorer",
                center.x - 110.,
                center.y + 55.,
                22.,
                WHITE,
            );
        }
        transport.flush().map_err(|e| e.to_string())?;
        if own.is_some() {
            frame += 1;
        }
        if options.net_report.is_some() && frame > 120 {
            metrics.record(get_frame_time());
        }
        if !captured_peer
            && visible_peers > 0
            && frame >= 120
            && let Some(path) = &options.capture
        {
            let stem = path.file_stem().unwrap_or_default().to_string_lossy();
            crate::capture(&path.with_file_name(format!("{stem}-peer.png")))?;
            captured_peer = true;
        }
        if options.net_frames.is_some_and(|limit| frame >= limit) {
            if let Some(path) = &options.capture {
                crate::capture(path)?;
            }
            break;
        }
        next_frame().await;
    }
    if let Some(path) = &options.net_report {
        let mut report: serde_json::Value = serde_json::from_str(&metrics.json(
            options.width,
            options.height,
            terrain.len(),
            terrain.faces,
            "greedy",
        ))
        .map_err(|e| e.to_string())?;
        report["mode"] = "authoritative multiplayer".into();
        report["server"] = address.into();
        report["connection_id"] = transport.connection_id().0.into();
        report["actor_id"] = serde_json::json!(own_id);
        report["position"] = serde_json::json!(own.as_ref().map(|a| a.position));
        report["peer_ids"] = serde_json::json!(peer_ids);
        report["peer_positions"] = serde_json::json!(
            remotes
                .values()
                .map(|r| (&r.actor.id, r.actor.position))
                .collect::<Vec<_>>()
        );
        report["max_peers"] = max_peers.into();
        report["max_visible_peers"] = max_visible_peers.into();
        report["changed_peer_poses"] = changed_peer_poses.into();
        report["snapshots"] = snapshots.into();
        report["accepted_edits"] = accepted_edits.into();
        report["rejected_edits"] = rejected_edits.into();
        report["maximum_quads"] = maximum_quads.into();
        report["maximum_mesh_chunks"] = maximum_chunks.into();
        report["first_geometry_ms"] = serde_json::json!(ready_at);
        report["rtt_mean_ms"] =
            serde_json::json!(rtt_samples.iter().sum::<f64>() / rtt_samples.len().max(1) as f64);
        report["frames"] = frame.into();
        report["elapsed_seconds"] = start.elapsed().as_secs_f64().into();
        report["route"] = "clock-driven circular flight; opposing actor-id parity; authoritative positions; 120 warmup frames".into();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn interpolation_preserves_authoritative_endpoints() {
        assert_eq!(
            super::interpolate([1., 2., 3.], [5., 4., 9.], 0.),
            [1., 2., 3.]
        );
        assert_eq!(
            super::interpolate([1., 2., 3.], [5., 4., 9.], 1.),
            [5., 4., 9.]
        );
        assert_eq!(
            super::interpolate([1., 2., 3.], [5., 4., 9.], 0.5),
            [3., 3., 6.]
        );
    }
}
