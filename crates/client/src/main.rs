mod autosave;
mod metrics;
mod options;
mod profile;
mod terrain;

use macroquad::prelude::*;
use metrics::FrameMetrics;
use options::Options;
use primal_simulation::{Edit, FIXED_DT, Input, MAX_REACH, Player, edit, raycast};
use primal_storage::{WorldLock, load_or_create};
use primal_voxel::{Pos, dirty_chunks};
use profile::Profile;
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
    let result = run(&options).await;
    if cleanup_temporary_world {
        let _ = std::fs::remove_file(&options.world);
        let mut lock_path = options.world.as_os_str().to_owned();
        lock_path.push(".lock");
        let _ = std::fs::remove_file(lock_path);
    }
    if let Err(error) = result {
        eprintln!("PRIMAL VOXEL ERROR {error}");
        if options.smoke {
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

async fn run(options: &Options) -> Result<(), String> {
    if options.smoke && options.world.exists() {
        return Err(format!(
            "Le test smoke exige une sauvegarde neuve. Fichier existant {}",
            options.world.display()
        ));
    }
    clear_background(Color::from_rgba(22, 36, 33, 255));
    draw_text("PRIMAL VOXEL", 40.0, 65.0, 34.0, WHITE);
    draw_text("Génération de la vallée…", 40.0, 102.0, 20.0, WHITE);
    next_frame().await;

    let _lock = WorldLock::acquire(&options.world)
        .map_err(|error| format!("{}\n{error}", options.world.display()))?;
    let mut world = load_or_create(&options.world, SEED)
        .map_err(|error| format!("{}\n{error}", options.world.display()))?;
    let mut writer = autosave::Autosave::new(options.world.clone()).map_err(|e| e.to_string())?;
    let mut player = Player::try_spawn(&world)
        .ok_or("Aucun espace libre pour apparaître dans cette sauvegarde")?;
    let mut terrain = Terrain::new(&world, options.mesher);
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
    let mut distance: f32 = 112.0;
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

    loop {
        if let Some(ack) = writer.poll(&world).map_err(|e| e.to_string())? {
            profile.sample(ack.frame, "save_queue", ack.wait);
            profile.sample(ack.frame, "save_disk", ack.disk);
            match ack.result {
                Ok(()) => {
                    status = "Sauvegarde confirmée".into();
                    save_failed = false;
                }
                Err(e) => {
                    status = format!("ÉCHEC SAUVEGARDE · {e} · F5 pour réessayer");
                    save_failed = true;
                }
            }
            status_until = get_time() + 3.0;
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
            if save_failed {
                writer.request(&world, frame).map_err(|e| e.to_string())?;
            }
            if let Err(e) = writer.flush(&world) {
                save_failed = true;
                status = format!("ÉCHEC SAUVEGARDE · {e} · F5 pour réessayer");
            } else {
                break;
            }
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
        }
        if is_key_pressed(KeyCode::Minus) {
            distance = (distance - 16.0).max(32.0);
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
            player.flying = true;
            yaw = -0.35
                + ((frame as f32 / options.frames as f32) * std::f32::consts::TAU).sin() * 0.38;
            pitch = -0.13;
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
            if frame > 60 {
                input.movement = (forward * 0.14 + right * 0.05).to_array();
            }
            player.step(&world, input, FIXED_DT);
        } else {
            accumulator += frame_dt.min(0.1);
            while accumulator >= fixed_dt {
                input.jump = jump_pending;
                player.step(&world, input, fixed_dt);
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
            match edit(&mut world, &player, front.to_array(), operation) {
                Ok(position) => {
                    for chunk in dirty_chunks(position) {
                        terrain.rebuild(&world, chunk);
                    }
                    writer.request(&world, frame).map_err(|e| e.to_string())?;
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
            writer.request(&world, frame).map_err(|e| e.to_string())?;
            status = "Sauvegarde demandée".into();
            status_until = get_time() + 4.0;
        }
        if options.smoke && frame == 20 {
            let direction = [0.0, -0.85, 0.5];
            let hit = raycast(&world, player.eye(), direction, MAX_REACH)
                .ok_or("Smoke: aucune cible terrain")?;
            let block = world.get(hit.block);
            let pos = edit(&mut world, &player, direction, Edit::Remove)
                .map_err(|error| format!("Smoke destruction {error:?}"))?;
            for chunk in dirty_chunks(pos) {
                terrain.rebuild(&world, chunk);
            }
            writer
                .request(&world, frame)
                .and_then(|()| writer.flush(&world))
                .map_err(|e| e.to_string())?;
            let loaded = load_or_create(&options.world, SEED).map_err(|error| error.to_string())?;
            if loaded.get(pos) != 0 {
                return Err("Smoke: destruction perdue après rechargement".into());
            }
            smoke_removed = Some((pos, block));
            println!("SMOKE remove/save/load verified at {pos:?}");
        }
        if options.smoke && frame == 40 {
            let (expected, block) = smoke_removed.ok_or("Smoke: étape destruction absente")?;
            let pos = edit(&mut world, &player, [0.0, -0.85, 0.5], Edit::Place(block))
                .map_err(|error| format!("Smoke pose {error:?}"))?;
            if pos != expected {
                return Err(format!("Smoke: pose {pos:?}, attendu {expected:?}"));
            }
            for chunk in dirty_chunks(pos) {
                terrain.rebuild(&world, chunk);
            }
            writer
                .request(&world, frame)
                .and_then(|()| writer.flush(&world))
                .map_err(|e| e.to_string())?;
            let loaded = load_or_create(&options.world, SEED).map_err(|error| error.to_string())?;
            if loaded.get(pos) != block {
                return Err("Smoke: pose perdue après rechargement".into());
            }
            smoke_restored = true;
            println!("SMOKE place/save/load verified at {pos:?}");
        }

        if options.edit_route && frame >= 120 && frame.is_multiple_of(12) {
            // Paired removals/restorations across interior and chunk-edge columns.
            let pair = (frame - 120) / 12;
            let columns = [(-17, -9), (-16, -9), (-8, -9), (0, -1), (15, 4), (16, 4)];
            let (x, z) = columns[(pair / 2) % columns.len()];
            let start = Instant::now();
            let surface = (1..60)
                .rev()
                .find(|&y| world.get([x, y, z]) != 0)
                .ok_or("Route édition: colonne vide")?;
            let mut actor = player.clone();
            actor.position = [x as f32 + 0.5, surface as f32 + 2.0, z as f32 + 0.5];
            let action = if pair.is_multiple_of(2) {
                route_block = world.get([x, surface, z]);
                Edit::Remove
            } else {
                Edit::Place(route_block)
            };
            let pos = edit(&mut world, &actor, [0.0, -1.0, 0.0], action)
                .map_err(|e| format!("Route édition {e}"))?;
            profile.sample(frame, "edit_command", start.elapsed());
            for chunk in dirty_chunks(pos) {
                let (mesh, buffers) = terrain.rebuild(&world, chunk);
                profile.sample(frame, "remesh", mesh);
                profile.sample(frame, "prepare_buffers", buffers);
            }
            let disk = Instant::now();
            writer.request(&world, frame).map_err(|e| e.to_string())?;
            profile.sample(frame, "save_enqueue", disk.elapsed());
            let border = pos.iter().any(|v| matches!(v.rem_euclid(16), 0 | 15));
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
        let visible = terrain.draw(eye, front, distance);
        profile.sample(frame, "draw_submit_cpu", draw_start.elapsed());
        gl_use_default_material();
        if let Some(hit) = raycast(&world, player.eye(), front.to_array(), MAX_REACH) {
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
        set_default_camera();
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
    writer.flush(&world).map_err(|e| e.to_string())?;
    if let Some(path) = &options.profile {
        profile.write(path).map_err(|e| e.to_string())?;
    }
    set_cursor_grab(false);
    show_mouse(true);
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
