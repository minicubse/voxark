use macroquad::prelude::*;
use std::{
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
};

pub enum Choice {
    Solo,
    Host(String),
    Join(String),
    Quit,
}

pub async fn choose(error: Option<&str>) -> Choice {
    set_cursor_grab(false);
    show_mouse(true);
    let mut address = "127.0.0.1:47620".to_owned();
    let mut editing = false;
    loop {
        clear_background(Color::from_rgba(22, 36, 33, 255));
        let x = (screen_width() - 480.) * 0.5;
        let y = (screen_height() - 440.).max(40.) * 0.5;
        draw_text("PRIMAL VOXEL", x, y + 30., 42., WHITE);
        draw_text("La vallée à partager", x, y + 65., 22., LIGHTGRAY);
        if button("Solo", x, y + 95., 480., false) {
            return Choice::Solo;
        }
        if button("Héberger une partie", x, y + 153., 480., false) {
            return Choice::Host(address);
        }
        draw_text("Adresse du serveur (IP:port)", x, y + 230., 18., LIGHTGRAY);
        if button(&address, x, y + 243., 480., editing) {
            editing = true;
        }
        if editing {
            while let Some(c) = get_char_pressed() {
                if !c.is_control() && address.len() < 100 {
                    address.push(c);
                }
            }
            if is_key_pressed(KeyCode::Backspace) {
                address.pop();
            }
            if is_key_pressed(KeyCode::Enter) {
                return Choice::Join(address);
            }
        }
        if button("Rejoindre", x, y + 301., 480., false) {
            return Choice::Join(address);
        }
        if let Some(error) = error {
            for (i, line) in crate::wrap(error, 65).iter().take(4).enumerate() {
                draw_text(line, x, y + 390. + i as f32 * 22., 18., ORANGE);
            }
        }
        draw_text("Échap : quitter", x, screen_height() - 24., 18., LIGHTGRAY);
        if is_key_pressed(KeyCode::Escape) || is_quit_requested() {
            return Choice::Quit;
        }
        next_frame().await;
    }
}

fn button(label: &str, x: f32, y: f32, width: f32, active: bool) -> bool {
    let hovered = Rect::new(x, y, width, 48.).contains(Vec2::from(mouse_position()));
    let color = if hovered || active {
        Color::from_rgba(73, 103, 76, 255)
    } else {
        Color::from_rgba(35, 59, 47, 255)
    };
    draw_rectangle(x, y, width, 48., color);
    draw_text(label, x + 18., y + 32., 24., WHITE);
    hovered && is_mouse_button_pressed(MouseButton::Left)
}

/// Own the child: a normal departure asks the server to flush before exiting.
pub struct HostedServer(Child);

impl HostedServer {
    pub fn start(address: &str, world: &Path) -> Result<Self, String> {
        let address: std::net::SocketAddr =
            address.parse().map_err(|_| "Adresse attendue : IP:port")?;
        let bind = std::net::SocketAddr::new(
            if address.is_ipv4() {
                std::net::Ipv4Addr::UNSPECIFIED.into()
            } else {
                std::net::Ipv6Addr::UNSPECIFIED.into()
            },
            address.port(),
        );
        let directory = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .parent()
            .ok_or("Dossier du client absent")?
            .to_owned();
        let executable = ["primal-server.exe", "PrimalServer.exe", "primal-server"]
            .into_iter()
            .map(|name| directory.join(name))
            .find(|p| p.is_file())
            .ok_or("PrimalServer.exe ou primal-server.exe doit être placé à côté du client")?;
        let mut command = Command::new(executable);
        command
            .args(["--bind", &bind.to_string(), "--world"])
            .arg(world)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        command
            .spawn()
            .map(Self)
            .map_err(|e| format!("Démarrage du serveur : {e}"))
    }
    pub fn check_running(&mut self) -> Result<(), String> {
        if let Some(status) = self.0.try_wait().map_err(|e| e.to_string())? {
            let mut reason = String::new();
            if let Some(mut stderr) = self.0.stderr.take() {
                let _ = stderr.read_to_string(&mut reason);
            }
            return Err(format!("Serveur arrêté ({status}) : {reason}"));
        }
        Ok(())
    }
    pub fn stop(&mut self) -> Result<(), String> {
        if self.0.try_wait().map_err(|e| e.to_string())?.is_some() {
            return Ok(());
        }
        if let Some(mut stdin) = self.0.stdin.take() {
            stdin.write_all(b"stop\n").map_err(|e| e.to_string())?;
        }
        let start = std::time::Instant::now();
        while self.0.try_wait().map_err(|e| e.to_string())?.is_none() {
            if start.elapsed().as_secs() >= 18 {
                return Err("Le serveur n'a pas confirmé son arrêt sous 18 s".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        Ok(())
    }
}
impl Drop for HostedServer {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
