#[cfg(test)]
mod autosave;
mod buffers;
mod menu;
mod metrics;
mod online;
mod options;
mod profile;
mod scenarios;
mod terrain;

use buffers::Buffers;
use macroquad::prelude::*;
use metrics::FrameMetrics;
use options::Options;
use primal_simulation::{Edit, FIXED_DT, Input, MAX_REACH, Player, edit, raycast};
use primal_storage::RegionStore;
use primal_streaming::{MeshKind, Streamer, View};
use primal_voxel::{Pos, World, chunk_at};
use profile::Profile;
use scenarios::{Burst, Scenario};
use std::path::Path;
use std::time::Instant;
use terrain::{FOG_COLOR, Terrain, block_color};

const SEED: u64 = 20_261_120;

fn window_conf() -> macroquad::conf::Conf {
    let options = Options::parse(std::env::args().skip(1)).ok();
    macroquad::conf::Conf {
        miniquad_conf: Conf {
            window_title: "PRIMAL VOXEL — Vallée originelle".into(),
            window_width: options.as_ref().map_or(1280, |options| options.width),
            window_height: options.as_ref().map_or(720, |options| options.height),
            sample_count: 1,
            high_dpi: false,
            ..Default::default()
        },
        draw_call_vertex_capacity: 32_768,
        draw_call_index_capacity: 49_152,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    prevent_quit();
    let options = match Options::parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}");
            if std::env::args().any(|arg| arg == "--help" || arg == "-h") {
                return;
            }
            std::process::exit(2);
        }
    };
    let cleanup_temporary_world = options.temporary_world && !options.world.exists();
    let result = if let Some(address) = &options.connect {
        online::run(&options, address).await
    } else if options.smoke || options.direct_solo {
        run(&options).await
    } else {
        run_menu(&options).await
    };
    if cleanup_temporary_world {
        // Options creates a unique private temp directory for this smoke invocation.
        if let Some(dir) = options.world.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
    if let Err(error) = result {
        eprintln!("PRIMAL VOXEL ERROR {error}");
        if options.smoke || options.net_frames.is_some() {
            std::process::exit(1);
        }
        loop {
            clear_background(Color::from_rgba(22, 36, 33, 255));
            draw_text("Ouverture du monde impossible", 40.0, 70.0, 30.0, WHITE);
            for (index, line) in wrap(&error, 100).iter().enumerate() {
                draw_text(
                    line,
                    40.0,
                    115.0 + index as f32 * 22.0,
                    19.0,
                    Color::from_rgba(244, 194, 147, 255),
                );
            }
            draw_text(
                "Consultez le terminal et la sauvegarde indiquée. Échap pour fermer.",
                40.0,
                screen_height() - 45.0,
                19.0,
                WHITE,
            );
            if is_key_pressed(KeyCode::Escape) || is_quit_requested() {
                break;
            }
            next_frame().await;
        }
    }
}

async fn run_menu(options: &Options) -> Result<(), String> {
    let mut error = None;
    loop {
        let result = match menu::choose(error.as_deref()).await {
            menu::Choice::Quit => return Ok(()),
            menu::Choice::Solo => run(options).await,
            menu::Choice::Join(address) => online::run(options, &address).await,
            menu::Choice::Host(address) => {
                match menu::HostedServer::start(&address, &options.world) {
                    Ok(mut server) => {
                        // Startup includes the region store and socket; yield frames while it boots.
                        let start = get_time();
                        while get_time() - start < 0.8 {
                            clear_background(FOG_COLOR);
                            draw_text("Démarrage du serveur…", 40., 70., 28., WHITE);
                            next_frame().await;
                        }
                        let played = online::run(options, &address).await;
                        let stopped = server.stop();
                        played.and(stopped)
                    }
                    Err(error) => Err(error),
                }
            }
        };
        error = result.err();
        if is_quit_requested() {
            return Ok(());
        }
        next_frame().await;
    }
}

async fn run(options: &Options) -> Result<(), String> {
    if options.smoke && (options.world.exists() || RegionStore::directory(&options.world).exists())
    {
        return Err(format!(
            "Le test smoke exige une sauvegarde neuve. Fichier existant {}",
            options.world.display()
        ));
    }
    clear_background(Color::from_rgba(22, 36, 33, 255));
    draw_text("PRIMAL VOXEL", 40.0, 65.0, 34.0, WHITE);
    draw_text("Génération de la vallée…", 40.0, 102.0, 20.0, WHITE);
    next_frame().await;

    let store = RegionStore::open(&options.world, SEED)
        .map_err(|e| format!("{} {e}", options.world.display()))?;
    let kind = match options.mesher {
        options::Mesher::Greedy => MeshKind::Greedy,
        options::Mesher::Binary => MeshKind::Binary,
        options::Mesher::Faces => MeshKind::Faces,
    };
    let mut stream = Streamer::new(
        if options.valley_only {
            World::valley_streamed(store.seed())
        } else {
            World::streamed(store.seed())
        },
        store.clone(),
        options.stream.clone(),
        kind,
        Buffers::prepare,
    )
    .map_err(|e| e.to_string())?;
    let mut terrain = Terrain::default();
    let mut view = View {
        block: [0, 16, 0],
        forward: [0., 0., 1.],
        velocity: [0.; 3],
    };
    let boot = Instant::now();
    let mut player = loop {
        let f = stream.tick(view);
        if let Some(e) = f.errors.first() {
            return Err(e.clone());
        }
        for u in f.uploads {
            terrain.install(u.chunk, u.payload);
        }
        if (0..stream.world().block_bounds().1[1] as i32 / 16)
            .all(|y| stream.world().is_resident([0, y, 0]))
            && let Some(p) = Player::try_spawn(stream.world())
        {
            break p;
        }
        if boot.elapsed().as_secs() > 12 {
            return Err("Zone de départ indisponible sous les budgets configurés".into());
        }
        next_frame().await;
    };
    let fog = match terrain::fog_material() {
        Ok(material) => Some(material),
        Err(error) => {
            eprintln!("Brouillard indisponible, rendu standard actif. {error}");
            None
        }
    };
    let mut yaw: f32 = -0.35;
    let mut pitch: f32 = -0.12;
    let mut captured = false;
    let mut selected = 1_u8;
    let mut debug = true;
    let mut fullscreen = false;
    let mut distance = (options.stream.render_radius as f32 * 16.0).max(16.0);
    let mut accumulator = 0.0;
    let fixed_dt = 1.0 / f32::from(options.tick_rate);
    let mut jump_pending = false;
    let mut frame = 0_usize;
    let mut metrics = FrameMetrics::default();
    let mut profile = Profile::new(options.profile.is_some());
    let mut status = "Monde chargé · sauvegarde à chaque modification".to_owned();
    let mut status_until = get_time() + 5.0;
    let mut save_failed = false;
    let mut smoke_removed: Option<(Pos, u8)> = None;
    let mut smoke_restored = false;
    let mut captured_image = false;
    let started = get_time();
    let mut route_block = 0;

    let mut awaiting_save = false;
    let mut explicit_save = false;
    let mut closing = false;
    let mut burst = None;
    let first_usable_ms = boot.elapsed().as_secs_f64() * 1000.;
    let mut missing_ground_frames = 0;
    let mut probe = scenarios::GroundProbe::default();
    let mut missing_data = 0;
    let mut missing_mesh = 0;
    let mut strict_run = 0;
    let mut longest_strict = 0;
    let mut missing_run = 0;
    let mut longest_missing = 0;
    let mut collision_missing = 0;
    let mut distance_travelled = 0.;
    let mut last_position = None;
    let mut edited = std::collections::BTreeMap::new();
    let mut checkpoints = std::collections::HashSet::new();
    let mut arrivals = std::collections::HashMap::new();
    let mut pending_edits: Vec<(Pos, Instant, bool, bool)> = Vec::new();
    loop {
        if options.smoke
            && let Some(flight) = options.scenario.flight(frame, options.frames)
        {
            player.position = flight.position;
            player.velocity = flight.velocity;
            yaw = flight.yaw;
            pitch = -0.45;
        }
        view = View {
            block: player.position.map(|x| x.floor() as i32),
            forward: [
                yaw.sin() * pitch.cos(),
                pitch.sin(),
                yaw.cos() * pitch.cos(),
            ],
            velocity: player.velocity,
        };
        let t = Instant::now();
        let f = stream.tick(view);
        profile.sample(frame, "stream_tick", t.elapsed());
        for sample in f.timings {
            profile.sample(frame, sample.stage.name(), sample.duration);
        }
        for c in f.evicted {
            terrain.remove(c);
        }
        let adopt = Instant::now();
        for u in f.uploads {
            if stream.world().mesh_revision(u.chunk) == Some(u.revision) {
                terrain.install(u.chunk, u.payload);
            }
        }
        profile.sample(frame, "accept_buffers_cpu", adopt.elapsed());
        observe_edits(&mut pending_edits, &stream, &mut profile, frame);
        if let Some(e) = f.errors.first() {
            if options.smoke {
                return Err(e.clone());
            }
            save_failed = true;
            closing = false;
            status = format!("Erreur de streaming · {e} · F5 pour réessayer");
        }
        if awaiting_save && stream.saves_idle() {
            awaiting_save = false;
            save_failed = false;
            status = "Sauvegarde confirmée".into();
            status_until = get_time() + 3.;
            if explicit_save {
                println!("F5 SAVE CONFIRMED");
                explicit_save = false;
            }
            if closing {
                break;
            }
        }
        if frame >= scenarios::WARMUP {
            if options.smoke {
                let here = Vec3::from_array(player.position);
                if let Some(previous) = last_position {
                    distance_travelled += here.distance(previous);
                }
                last_position = Some(here);
                let began = Instant::now();
                if !player.collision_data_ready(stream.world()) {
                    collision_missing += 1;
                }
                profile.sample(frame, "collision_probe", began.elapsed());
                let [x, _, z] = view.block;
                let surface = chunk_at(probe.surface(stream.world(), x, z));
                if !stream.world().is_resident(surface) {
                    missing_data += 1;
                    strict_run += 1;
                } else if !terrain.has_geometry(surface) {
                    missing_mesh += 1;
                    strict_run += 1;
                } else {
                    strict_run = 0;
                }
                longest_strict = longest_strict.max(strict_run);
                let ground = (0..128)
                    .rev()
                    .find(|&y| stream.world().lookup([x, y, z]).is_some_and(|b| b > 0));
                if !ground.is_some_and(|y| stream.visible_revision(chunk_at([x, y, z])).is_some()) {
                    missing_ground_frames += 1;
                    missing_run += 1;
                    longest_missing = longest_missing.max(missing_run);
                } else {
                    missing_run = 0;
                }
            }
            for &[x, z] in options.scenario.checkpoints() {
                if (view.block[0] - x).abs() >= 16
                    || (view.block[2] - z).abs() >= 16
                    || checkpoints.contains(&[x, z])
                {
                    continue;
                }
                // The teleport route returns to origin after its distant leg.
                if options.scenario == Scenario::Return
                    && x == 0
                    && !checkpoints.contains(&[256, 64])
                {
                    continue;
                }
                if options.scenario == Scenario::Teleport
                    && x == 0
                    && !checkpoints.contains(&[65_536, -32_768])
                {
                    continue;
                }
                let arrival = arrivals.entry([x, z]).or_insert_with(Instant::now);
                if let Some(y) = (0..128)
                    .rev()
                    .find(|&y| stream.world().lookup([x, y, z]).is_some_and(|b| b > 0))
                    && stream.visible_revision(chunk_at([x, y, z])).is_some()
                {
                    profile.sample(frame, "arrival_to_ground_mesh", arrival.elapsed());
                    checkpoints.insert([x, z]);
                }
            }
        }
        let frame_dt = get_frame_time();
        if options.smoke && frame >= 120 {
            metrics.record(frame_dt);
            profile.sample(
                frame - 1,
                "frame_interval",
                std::time::Duration::from_secs_f32(frame_dt),
            );
        }
        if is_quit_requested() {
            if options.smoke {
                return Err("Smoke interrompu avant validation".into());
            }
            stream.retry_errors();
            awaiting_save = true;
            closing = true;
        }
        if is_key_pressed(KeyCode::Escape) {
            captured = false;
            set_cursor_grab(false);
            show_mouse(true);
        }
        let just_captured =
            !options.smoke && !captured && is_mouse_button_pressed(MouseButton::Left);
        if just_captured {
            captured = true;
            set_cursor_grab(true);
            show_mouse(false);
        }
        if is_key_pressed(KeyCode::F3) {
            debug = !debug;
        }
        if is_key_pressed(KeyCode::F11) {
            fullscreen = !fullscreen;
            set_fullscreen(fullscreen);
        }
        if is_key_pressed(KeyCode::F) {
            player.flying = !player.flying;
        }
        if is_key_pressed(KeyCode::Equal) {
            distance = (distance + 16.0).min(192.0);
            stream.set_render_radius((distance / 16.).ceil() as i32);
        }
        if is_key_pressed(KeyCode::Minus) {
            distance = (distance - 16.0).max(32.0);
            stream.set_render_radius((distance / 16.).ceil() as i32);
        }
        for (index, key) in [
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
                selected = index as u8 + 1;
            }
        }
        let wheel = mouse_wheel().1;
        if wheel != 0.0 {
            selected = ((i32::from(selected) - 1 - wheel.signum() as i32).rem_euclid(7) + 1) as u8;
        }
        if captured && !just_captured {
            let delta = mouse_delta_position();
            yaw += delta.x * 2.5;
            pitch = (pitch + delta.y * 2.0).clamp(-1.52, 1.52);
        }
        if options.smoke {
            player.flying =
                frame < 120 || !matches!(options.scenario, Scenario::Walk | Scenario::Sprint);
            if options.scenario.flight(frame, options.frames).is_none() {
                yaw = -0.35
                    + ((frame as f32 / options.frames as f32) * std::f32::consts::TAU).sin() * 0.38;
                pitch = -0.13;
            }
        }
        let front = vec3(
            yaw.sin() * pitch.cos(),
            pitch.sin(),
            yaw.cos() * pitch.cos(),
        );
        let forward = vec3(yaw.sin(), 0.0, yaw.cos());
        let right = forward.cross(Vec3::Y);
        let mut input = Input::default();
        if captured {
            let forward_amount = axis(
                is_key_down(KeyCode::W) || is_key_down(KeyCode::Z) || is_key_down(KeyCode::Up),
                is_key_down(KeyCode::S) || is_key_down(KeyCode::Down),
            );
            let right_amount = axis(
                is_key_down(KeyCode::D) || is_key_down(KeyCode::Right),
                is_key_down(KeyCode::A) || is_key_down(KeyCode::Q) || is_key_down(KeyCode::Left),
            );
            let vertical = axis(
                is_key_down(KeyCode::Space),
                is_key_down(KeyCode::LeftControl),
            );
            input.movement =
                (forward * forward_amount + right * right_amount + Vec3::Y * vertical).to_array();
            input.sprint = is_key_down(KeyCode::LeftShift);
            jump_pending |= is_key_pressed(KeyCode::Space);
        }
        if options.smoke {
            if frame > 60 && options.scenario.flight(frame, options.frames).is_none() {
                input.movement = (forward * 0.14 + right * 0.05).to_array();
            }
            if frame >= 120
                && matches!(
                    options.scenario,
                    Scenario::Walk | Scenario::Sprint | Scenario::Fly | Scenario::MovingEdits
                )
            {
                input.movement = [0., 0., 1.];
                input.sprint = options.scenario == Scenario::Sprint;
                let [px, py, pz] = player.position;
                // Jump only at a step on the walking route; use the same collision solver as keyboard input.
                input.jump = !player.flying
                    && player.grounded
                    && stream
                        .world()
                        .lookup([
                            px.floor() as i32,
                            py.floor() as i32,
                            (pz + 0.65).floor() as i32,
                        ])
                        .is_some_and(|b| b != 0);
                if player.flying && frame < 180 {
                    input.movement[1] = 0.35;
                }
            }
            if options.scenario.flight(frame, options.frames).is_none() {
                player.step(stream.world(), input, FIXED_DT);
            }
        } else {
            accumulator += frame_dt.min(0.1);
            while accumulator >= fixed_dt {
                input.jump = jump_pending;
                player.step(stream.world(), input, fixed_dt);
                jump_pending = false;
                accumulator -= fixed_dt;
            }
        }

        let operation = if captured && !just_captured && is_mouse_button_pressed(MouseButton::Left)
        {
            Some(Edit::Remove)
        } else if captured && is_mouse_button_pressed(MouseButton::Right) {
            Some(Edit::Place(selected))
        } else {
            None
        };
        if let Some(operation) = operation {
            match edit(stream.world_mut(), &player, front.to_array(), operation) {
                Ok(_) => {
                    awaiting_save = true;
                    status = "Modification en cours de sauvegarde".into();
                }
                Err(error) => {
                    if !save_failed {
                        status = format!("Action refusée · {error:?}");
                    }
                }
            }
            status_until = get_time() + 3.0;
        }
        if is_key_pressed(KeyCode::F5) {
            stream.retry_errors();
            awaiting_save = true;
            explicit_save = true;
            status = "Sauvegarde demandée".into();
            status_until = get_time() + 4.0;
        }
        if options.smoke && frame == 20 {
            let direction = [0.0, -0.85, 0.5];
            let hit = raycast(stream.world(), player.eye(), direction, MAX_REACH)
                .ok_or("Smoke: aucune cible terrain")?;
            let block = stream.world().get(hit.block);
            let pos = edit(stream.world_mut(), &player, direction, Edit::Remove)
                .map_err(|error| format!("Smoke destruction {error:?}"))?;
            verify_write(&mut stream, &store, &mut terrain, view, pos, 0).await?;
            if stream.world().get(pos) != 0 {
                return Err("Smoke: destruction perdue après rechargement".into());
            }
            smoke_removed = Some((pos, block));
            println!("SMOKE remove/save/load verified at {pos:?}");
        }
        if options.smoke && frame == 40 {
            let (expected, block) = smoke_removed.ok_or("Smoke: étape destruction absente")?;
            let pos = edit(
                stream.world_mut(),
                &player,
                [0.0, -0.85, 0.5],
                Edit::Place(block),
            )
            .map_err(|error| format!("Smoke pose {error:?}"))?;
            if pos != expected {
                return Err(format!("Smoke: pose {pos:?}, attendu {expected:?}"));
            }
            verify_write(&mut stream, &store, &mut terrain, view, pos, block).await?;
            if stream.world().get(pos) != block {
                return Err("Smoke: pose perdue après rechargement".into());
            }
            smoke_restored = true;
            println!("SMOKE place/save/load verified at {pos:?}");
        }

        if options.scenario == Scenario::MovingEdits && frame >= 120 && frame.is_multiple_of(30) {
            let start = Instant::now();
            let x = player.position[0].floor() as i32 + 2;
            let z = player.position[2].floor() as i32;
            let y = (0..128)
                .rev()
                .find(|&y| stream.world().get([x, y, z]) > 0)
                .ok_or("Sol éditable absent")?;
            let mut actor = player.clone();
            actor.position = [x as f32 + 0.5, y as f32 + 1.01, z as f32 + 0.5];
            let pos = edit(stream.world_mut(), &actor, [0., -1., 0.], Edit::Remove)
                .map_err(|e| e.to_string())?;
            edited.insert(pos, 0u8);
            pending_edits.push((pos, start, false, false));
            profile.sample(frame, "moving_command", start.elapsed());
        }
        if options.edit_route && frame >= 120 && frame.is_multiple_of(12) {
            // Paired removals/restorations across interior and chunk-edge columns.
            let pair = (frame - 120) / 12;
            let columns = [(-17, -9), (-16, -9), (-8, -9), (0, -1), (15, 4), (16, 4)];
            let (x, z) = columns[(pair / 2) % columns.len()];
            let start = Instant::now();
            let surface = (1..60)
                .rev()
                .find(|&y| stream.world().get([x, y, z]) != 0)
                .ok_or_else(|| {
                    format!(
                        "Route édition: colonne vide ({x},{z}) frame {frame} en {:?}",
                        view.block
                    )
                })?;
            let mut actor = player.clone();
            actor.position = [x as f32 + 0.5, surface as f32 + 2.0, z as f32 + 0.5];
            let action = if pair.is_multiple_of(2) {
                route_block = stream.world().get([x, surface, z]);
                Edit::Remove
            } else {
                Edit::Place(route_block)
            };
            let pos = edit(stream.world_mut(), &actor, [0.0, -1.0, 0.0], action)
                .map_err(|e| format!("Route édition {e}"))?;
            profile.sample(frame, "edit_command", start.elapsed());
            pending_edits.push((pos, start, false, false));
            let border = scenarios::on_chunk_face(pos);
            profile.sample(
                frame,
                if border {
                    "edit_border"
                } else {
                    "edit_interior"
                },
                start.elapsed(),
            );
        }

        if options.scenario == Scenario::Edits && frame == 130 {
            burst = Some(Burst::remove(stream.world_mut(), |_, p, t| {
                pending_edits.push((p, t, false, false));
                profile.sample(frame, "burst_command", t.elapsed());
            })?);
        }
        if options.scenario == Scenario::Edits && frame == 160 {
            burst
                .as_ref()
                .ok_or("Rafale absente")?
                .restore(stream.world_mut(), |_, p, t| {
                    pending_edits.push((p, t, false, false));
                    profile.sample(frame, "burst_command", t.elapsed());
                })?;
        }
        let eye = Vec3::from_array(player.eye());
        draw_sky();
        set_camera(&Camera3D {
            position: eye,
            target: eye + front,
            up: Vec3::Y,
            fovy: terrain::VERTICAL_FOV,
            z_near: 0.05,
            z_far: 240.0,
            ..Default::default()
        });
        if let Some(fog) = &fog {
            fog.set_uniform("CameraPosition", eye);
            fog.set_uniform("FogDistance", distance);
            gl_use_material(fog);
        }
        let draw_start = Instant::now();
        let visible = terrain.draw(eye, front, distance, options.frustum);
        profile.sample(frame, "draw_submit_cpu", draw_start.elapsed());
        gl_use_default_material();
        if let Some(hit) = raycast(stream.world(), player.eye(), front.to_array(), MAX_REACH) {
            draw_cube_wires(
                Vec3::new(
                    hit.block[0] as f32 + 0.5,
                    hit.block[1] as f32 + 0.5,
                    hit.block[2] as f32 + 0.5,
                ),
                Vec3::splat(1.008),
                Color::from_rgba(245, 240, 191, 220),
            );
        }
        let flush = Instant::now();
        set_default_camera();
        profile.sample(frame, "backend_flush_upload_draw", flush.elapsed());
        draw_hud(
            &player,
            selected,
            captured || options.smoke,
            debug,
            visible,
            &terrain,
            distance,
            options.tick_rate,
        );
        if get_time() < status_until || save_failed {
            let color = if save_failed {
                Color::from_rgba(255, 163, 125, 255)
            } else {
                Color::from_rgba(233, 230, 200, 255)
            };
            let y = screen_height()
                - if screen_width() < 1000.0 {
                    205.0
                } else {
                    126.0
                };
            draw_text(&status, 26.0, y, 17.0, color);
        }
        if let Some(path) = &options.capture
            && !captured_image
            && frame == 60
        {
            capture(path)?;
            captured_image = true;
        }
        frame += 1;
        if options.smoke && frame >= options.frames {
            if !smoke_restored {
                return Err("Smoke incomplet".into());
            }
            if checkpoints.len() != options.scenario.checkpoints().len() {
                return Err(format!(
                    "Terrain non confirmé aux points attendus {:?}",
                    checkpoints
                ));
            }
            // Flush the tail outside the measured frame intervals, including short custom runs.
            let end = Instant::now();
            while !stream.saves_idle() || !pending_edits.is_empty() {
                let tail = stream.tick(view);
                if let Some(e) = tail.errors.first() {
                    return Err(e.clone());
                }
                for timing in tail.timings {
                    profile.sample(frame, timing.stage.name(), timing.duration);
                }
                for c in tail.evicted {
                    terrain.remove(c);
                }
                for u in tail.uploads {
                    terrain.install(u.chunk, u.payload);
                }
                observe_edits(&mut pending_edits, &stream, &mut profile, frame);
                if end.elapsed().as_secs() > 8 {
                    return Err("Le parcours n’a pas convergé après les dernières éditions".into());
                }
                if !stream.saves_idle() || !pending_edits.is_empty() {
                    next_frame().await;
                }
            }
            println!(
                "STREAM scenario={} checkpoints={} missing_ground_frames={missing_ground_frames} stats={:?} outstanding_edits={}",
                options.scenario.name(),
                checkpoints.len(),
                stream.stats(),
                pending_edits.len()
            );
            for (&p, &b) in &edited {
                if !store
                    .load_chunk(chunk_at(p))
                    .map_err(|e| e.to_string())?
                    .contains(&(p, b))
                {
                    return Err(format!("Édition perdue {p:?}"));
                }
            }
            println!(
                "PIPELINE missing_data={missing_data} missing_mesh={missing_mesh} longest_strict={longest_strict} first_usable_ms={first_usable_ms:.3} longest_missing={longest_missing} collision_missing={collision_missing} travel_blocks={distance_travelled:.2} disk_verified={}",
                edited.len()
            );
            let report = metrics.json(
                screen_width() as i32,
                screen_height() as i32,
                terrain.len(),
                terrain.faces,
                options.mesher.name(),
            );
            if let Some(path) = &options.metrics {
                metrics
                    .save(
                        path,
                        screen_width() as i32,
                        screen_height() as i32,
                        terrain.len(),
                        terrain.faces,
                        options.mesher.name(),
                    )
                    .map_err(|error| error.to_string())?;
            }
            println!(
                "SMOKE PASS frames={frame} elapsed_s={:.3}\n{report}",
                get_time() - started
            );
            break;
        }
        next_frame().await;
    }
    if !stream.saves_idle() {
        return Err("Sauvegardes encore en cours à la fin du parcours".into());
    }
    if let Some(path) = &options.profile {
        profile.write(path).map_err(|e| e.to_string())?;
    }
    set_cursor_grab(false);
    show_mouse(true);
    Ok(())
}

fn observe_edits(
    pending_edits: &mut Vec<(Pos, Instant, bool, bool)>,
    stream: &Streamer<Buffers>,
    profile: &mut Profile,
    frame: usize,
) {
    for (pos, started, visible, durable) in pending_edits.iter_mut() {
        let c = chunk_at(*pos);
        if !*visible
            && stream
                .world()
                .dirty_chunks(*pos)
                .iter()
                .filter(|c| stream.world().is_resident(**c))
                .all(|&c| stream.visible_revision(c) == stream.world().mesh_revision(c))
        {
            profile.sample(frame, "edit_to_mesh_ready", started.elapsed());
            *visible = true;
        }
        if !*durable && !stream.world().chunk_dirty(c) {
            profile.sample(frame, "edit_to_durable", started.elapsed());
            *durable = true;
        }
    }
    pending_edits.retain(|e| !e.2 || !e.3);
}

/// Smoke-only durability barrier. Reconstruct the edited block from the disk snapshot.
async fn verify_write(
    stream: &mut Streamer<Buffers>,
    store: &RegionStore,
    terrain: &mut Terrain,
    view: View,
    pos: Pos,
    expected: u8,
) -> Result<(), String> {
    let started = Instant::now();
    while !stream.saves_idle() {
        let f = stream.tick(view);
        if let Some(e) = f.errors.first() {
            return Err(e.clone());
        }
        for c in f.evicted {
            terrain.remove(c);
        }
        for u in f.uploads {
            if stream.world().mesh_revision(u.chunk) == Some(u.revision) {
                terrain.install(u.chunk, u.payload);
            }
        }
        if started.elapsed().as_secs() > 8 {
            return Err("Délai de sauvegarde dépassé".into());
        }
        next_frame().await;
    }
    let c = chunk_at(pos);
    let edits = store.load_chunk(c).map_err(|e| e.to_string())?;
    let value = edits
        .iter()
        .find(|(p, _)| *p == pos)
        .map(|e| e.1)
        .or_else(|| primal_voxel::generate_chunk(store.seed(), c)?.get(pos));
    if value != Some(expected) {
        return Err(format!(
            "Relecture de {pos:?} : {value:?}, attendu {expected}"
        ));
    }
    Ok(())
}

fn axis(positive: bool, negative: bool) -> f32 {
    i32::from(positive) as f32 - i32::from(negative) as f32
}

fn draw_sky() {
    clear_background(FOG_COLOR);
    let sky = Color::from_rgba(70, 127, 151, 255);
    let height = screen_height();
    let width = screen_width();
    let horizon = height * 0.57;
    draw_mesh(&Mesh {
        vertices: vec![
            Vertex::new(0.0, 0.0, 0.0, 0.0, 0.0, sky),
            Vertex::new(width, 0.0, 0.0, 0.0, 0.0, sky),
            Vertex::new(width, horizon, 0.0, 0.0, 0.0, FOG_COLOR),
            Vertex::new(0.0, horizon, 0.0, 0.0, 0.0, FOG_COLOR),
        ],
        indices: vec![0, 1, 2, 0, 2, 3],
        texture: None,
    });
}

#[allow(clippy::too_many_arguments)]
fn draw_hud(
    player: &Player,
    selected: u8,
    captured: bool,
    debug: bool,
    visible: usize,
    terrain: &Terrain,
    distance: f32,
    tick_rate: u16,
) {
    let width = screen_width();
    let height = screen_height();
    let ink = Color::from_rgba(244, 241, 214, 255);
    let panel = Color::from_rgba(15, 31, 28, 205);
    draw_rectangle(18.0, 18.0, 238.0, 72.0, panel);
    draw_rectangle(18.0, 18.0, 3.0, 72.0, Color::from_rgba(165, 192, 111, 255));
    draw_text("PRIMAL VOXEL", 33.0, 49.0, 28.0, ink);
    draw_text(
        "VALLÉE ORIGINELLE",
        34.0,
        74.0,
        15.0,
        Color::from_rgba(175, 195, 160, 255),
    );
    if debug {
        draw_rectangle(width - 247.0, 18.0, 229.0, 97.0, panel);
        draw_text(
            format!("{} FPS  /  {:.1} ms", get_fps(), get_frame_time() * 1000.0),
            width - 232.0,
            43.0,
            20.0,
            ink,
        );
        draw_text(
            format!(
                "{} / {} chunks · {} quads",
                visible,
                terrain.len(),
                terrain.faces
            ),
            width - 232.0,
            65.0,
            15.0,
            ink,
        );
        draw_text(
            format!(
                "{:.1}  {:.1}  {:.1}   portée {:.0} m",
                player.position[0], player.position[1], player.position[2], distance
            ),
            width - 232.0,
            86.0,
            15.0,
            ink,
        );
        let mode = if player.flying { "VOL" } else { "MARCHE" };
        draw_text(
            format!("{mode} · simulation {tick_rate} Hz"),
            width - 232.0,
            105.0,
            14.0,
            Color::from_rgba(175, 195, 160, 255),
        );
    }
    let center = vec2(width * 0.5, height * 0.5);
    for (dx, dy) in [(0.0, -7.0), (0.0, 7.0), (-7.0, 0.0), (7.0, 0.0)] {
        draw_circle(center.x + dx, center.y + dy, 1.4, ink);
    }
    draw_circle(center.x, center.y, 1.0, ink);
    let bar_width = 7.0 * 48.0;
    let start = (width - bar_width) * 0.5;
    let bar_y = height - 65.0;
    for id in 1..=7_u8 {
        let x = start + f32::from(id - 1) * 48.0;
        draw_rectangle(x, bar_y, 44.0, 46.0, panel);
        if id == selected {
            draw_rectangle_lines(
                x,
                bar_y,
                44.0,
                46.0,
                2.0,
                Color::from_rgba(225, 220, 161, 255),
            );
        }
        draw_rectangle(x + 13.0, bar_y + 9.0, 20.0, 20.0, block_color(id));
        draw_text(id.to_string(), x + 5.0, bar_y + 40.0, 13.0, ink);
    }
    let label =
        primal_voxel::block_def(selected).map_or("Bloc inconnu", |block| block.name.as_str());
    let label_width = measure_text(label, None, 20, 1.0).width;
    draw_text(label, (width - label_width) * 0.5, bar_y - 12.0, 20.0, ink);
    let controls_bottom = height - if width < 1000.0 { 97.0 } else { 18.0 };
    draw_rectangle(18.0, controls_bottom - 87.0, 285.0, 87.0, panel);
    draw_text(
        "ZQSD / WASD · espace · maj",
        30.0,
        controls_bottom - 63.0,
        16.0,
        ink,
    );
    draw_text(
        "Clic G détruire · clic D poser",
        30.0,
        controls_bottom - 41.0,
        16.0,
        ink,
    );
    draw_text(
        "F vol · F3 mesures · molette blocs",
        30.0,
        controls_bottom - 19.0,
        15.0,
        ink,
    );
    if !captured {
        let text = "Cliquez pour explorer · Échap libère la souris";
        let size = measure_text(text, None, 20, 1.0);
        draw_rectangle(
            (width - size.width) * 0.5 - 15.0,
            height * 0.67 - 28.0,
            size.width + 30.0,
            44.0,
            panel,
        );
        draw_text(text, (width - size.width) * 0.5, height * 0.67, 20.0, ink);
    }
    if width >= 1000.0 {
        draw_text(
            "F5 sauvegarder · F11 plein écran · +/- distance",
            width - 348.0,
            height - 88.0,
            14.0,
            ink,
        );
    } else {
        draw_text(
            "F5 sauvegarder · F11 plein écran",
            width - 266.0,
            controls_bottom - 59.0,
            14.0,
            ink,
        );
        draw_text(
            "+/- distance",
            width - 266.0,
            controls_bottom - 37.0,
            14.0,
            ink,
        );
    }
}

fn capture(path: &Path) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let path = path.to_str().ok_or("Chemin PNG non UTF-8")?;
    get_screen_data().export_png(path);
    println!("CAPTURE {path}");
    Ok(())
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    text.lines()
        .flat_map(|line| {
            line.chars()
                .collect::<Vec<_>>()
                .chunks(width)
                .map(|chunk| chunk.iter().collect::<String>())
                .collect::<Vec<_>>()
        })
        .collect()
}
