//! The UDP client pumps at the simulation rate even when rendering stalls.
use crate::{Channel, ClientMsg, ClientTransport, ConnectionId, ServerMsg};
use std::{
    net::{SocketAddr, ToSocketAddrs},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
#[derive(Default, Clone, Copy)]
pub struct Controls {
    pub movement: [f32; 3],
    pub jump: bool,
    pub sprint: bool,
    pub flying: bool,
    pub yaw: f32,
    pub pitch: f32,
    pub ready: bool,
}
pub struct Link {
    pub input: Arc<Mutex<Controls>>,
    pub events: mpsc::Receiver<ServerMsg>,
    pub id: ConnectionId,
    rtt_us: Arc<AtomicU64>,
    pub sent_inputs: Arc<AtomicU64>,
    commands: mpsc::SyncSender<ClientMsg>,
    thread: Option<thread::JoinHandle<()>>,
}
fn send(t: &mut ClientTransport, ch: Channel, msg: &ClientMsg) -> Result<(), String> {
    t.send(ch, crate::encode(msg).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
impl Link {
    pub fn open(addr: &str, token: [u8; 32], hz: u32) -> Result<Self, String> {
        if !(10..=120).contains(&hz) {
            return Err("Cadence serveur hors limites".into());
        }
        let server = addr
            .to_socket_addrs()
            .map_err(|e| e.to_string())?
            .next()
            .ok_or("Adresse inconnue")?;
        let bind: SocketAddr = if server.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        }
        .parse()
        .unwrap();
        let mut transport = ClientTransport::connect(
            bind,
            server,
            ConnectionId::random().map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let id = transport.connection_id();
        let rtt_us = Arc::new(AtomicU64::new(0));
        let timing = rtt_us.clone();
        let sent_inputs = Arc::new(AtomicU64::new(0));
        let counter = sent_inputs.clone();
        let (commands, rx) = mpsc::sync_channel(32);
        let (tx, events) = mpsc::sync_channel(1024);
        let input = Arc::new(Mutex::new(Controls::default()));
        let source = input.clone();
        let thread = thread::spawn(move || {
            let mut next = Instant::now();
            let mut previous = next;
            let mut hello = false;
            let mut seq = 0;
            let period = Duration::from_secs_f64(1. / f64::from(hz));
            let result = (|| -> Result<(), String> {
                loop {
                    let now = Instant::now();
                    transport.poll(now - previous).map_err(|e| e.to_string())?;
                    previous = now;
                    timing.store(transport.rtt().as_micros() as u64, Ordering::Relaxed);
                    if transport.is_connected() && !hello {
                        send(
                            &mut transport,
                            Channel::Control,
                            &ClientMsg::Hello {
                                version: crate::PROTOCOL_VERSION,
                                generator: primal_voxel::GENERATOR_VERSION,
                                token,
                            },
                        )?;
                        hello = true;
                    }
                    if let Some(reason) = transport.disconnect_reason() {
                        return Err(reason);
                    }
                    while let Some((_, bytes)) = transport.recv() {
                        let msg: ServerMsg = crate::decode(&bytes).map_err(|e| e.to_string())?;
                        if matches!(msg, ServerMsg::Welcome { .. }) {
                            send(&mut transport, Channel::Control, &ClientMsg::Ready)?;
                        }
                        tx.try_send(msg).map_err(|e| e.to_string())?;
                    }
                    loop {
                        match rx.try_recv() {
                            Ok(ClientMsg::Bye) | Err(mpsc::TryRecvError::Disconnected) => {
                                if transport.is_connected() {
                                    send(&mut transport, Channel::Control, &ClientMsg::Bye)?;
                                    let _ = transport.flush();
                                    transport.disconnect();
                                }
                                return Ok(());
                            }
                            Ok(msg) => send(&mut transport, Channel::Edits, &msg)?,
                            Err(mpsc::TryRecvError::Empty) => break,
                        }
                    }
                    let controls = {
                        let mut state = source.lock().unwrap();
                        let controls = *state;
                        state.jump = false;
                        controls
                    };
                    if hello && controls.ready {
                        seq += 1;
                        counter.fetch_add(1, Ordering::Relaxed);
                        send(
                            &mut transport,
                            Channel::Input,
                            &ClientMsg::Input {
                                seq,
                                movement: controls.movement,
                                jump: controls.jump,
                                sprint: controls.sprint,
                                flying: controls.flying,
                                yaw: controls.yaw,
                                pitch: controls.pitch,
                            },
                        )?;
                    }
                    transport.flush().map_err(|e| e.to_string())?;
                    next += period;
                    if next < Instant::now() {
                        next = Instant::now() + period;
                    }
                    thread::sleep(next.saturating_duration_since(Instant::now()));
                }
            })();
            if let Err(err) = result {
                eprintln!("Réseau : {err}");
            }
        });
        Ok(Self {
            input,
            events,
            commands,
            id,
            rtt_us,
            sent_inputs,
            thread: Some(thread),
        })
    }
    pub fn rtt_ms(&self) -> f64 {
        self.rtt_us.load(Ordering::Relaxed) as f64 / 1000.
    }
    pub fn edit(&self, msg: ClientMsg) -> Result<(), String> {
        self.commands.try_send(msg).map_err(|e| e.to_string())
    }
}
impl Drop for Link {
    fn drop(&mut self) {
        let _ = self.commands.send(ClientMsg::Bye);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
