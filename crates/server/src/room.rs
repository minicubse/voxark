use primal_net::{Actor, direction};
use primal_net::{
    Channel, ChunkData, ClientMsg, ConnectionId, PROTOCOL_VERSION, ServerEvent, ServerMsg,
    ServerTransport, decode, encode,
};
use primal_simulation::{Input, Player};
use primal_streaming::{Streamer, View};
use primal_voxel::{ChunkPos, GENERATOR_VERSION, World, chunk_at};
use std::{
    collections::{HashMap, HashSet},
    io,
    time::Duration,
};

struct Peer {
    id: u64,
    token: [u8; 32],
    ready: bool,
    player: Option<Player>,
    sent: HashMap<ChunkPos, u64>,
    arrival: Option<[f32; 3]>,
    input: Input,
    last_input: u64,
    sequence: u64,
    yaw: f32,
    pitch: f32,
    flying: bool,
}
impl Peer {
    fn view(&self) -> View {
        let block = self
            .player
            .as_ref()
            .map(|p| p.position)
            .or(self.arrival)
            .unwrap_or([0.5, 16., 0.5])
            .map(|v| v.floor() as i32);
        View {
            block,
            forward: direction(self.yaw, self.pitch),
            velocity: self.player.as_ref().map_or([0.; 3], |p| p.velocity),
        }
    }
}
pub struct Room {
    net: ServerTransport,
    peers: HashMap<ConnectionId, Peer>,
    next_id: u64,
    hello_deadlines: HashMap<ConnectionId, u64>,
    revisions: HashMap<ChunkPos, u64>,
}
impl Room {
    pub fn new(net: ServerTransport) -> Self {
        Self {
            net,
            peers: HashMap::new(),
            next_id: 1,
            hello_deadlines: HashMap::new(),
            revisions: HashMap::new(),
        }
    }
    fn message(
        &mut self,
        id: ConnectionId,
        channel: Channel,
        msg: ClientMsg,
        world: &World,
        tick: u64,
    ) {
        match (channel, msg) {
            (
                Channel::Control,
                ClientMsg::Hello {
                    version,
                    generator,
                    token,
                },
            ) if !self.peers.contains_key(&id) => {
                if version != PROTOCOL_VERSION
                    || generator != GENERATOR_VERSION
                    || token == [0; 32]
                    || self.peers.values().any(|p| p.token == token)
                {
                    let _ = self.net.send(
                        id,
                        Channel::Control,
                        encode(&ServerMsg::Error(
                            "Version incompatible ou identité déjà connectée".into(),
                        ))
                        .unwrap(),
                    );
                    return;
                }
                let actor_id = self.next_id;
                self.next_id += 1;
                let welcome = ServerMsg::Welcome {
                    id: actor_id,
                    seed: world.seed(),
                };
                if self
                    .net
                    .send(id, Channel::Control, encode(&welcome).unwrap())
                    .is_ok()
                {
                    self.hello_deadlines.remove(&id);
                    self.peers.insert(
                        id,
                        Peer {
                            id: actor_id,
                            token,
                            ready: false,
                            player: None,
                            sent: HashMap::new(),
                            arrival: None,
                            input: Input::default(),
                            last_input: 0,
                            sequence: 0,
                            yaw: -0.35,
                            pitch: -0.12,
                            flying: false,
                        },
                    );
                }
            }
            (Channel::Control, ClientMsg::Ready) => {
                if let Some(p) = self.peers.get_mut(&id) {
                    p.ready = true
                }
            }
            (
                Channel::Input,
                ClientMsg::Input {
                    seq,
                    movement,
                    jump,
                    sprint,
                    flying,
                    yaw,
                    pitch,
                },
            ) => {
                if let Some(p) = self.peers.get_mut(&id)
                    && p.ready
                    && seq > p.sequence
                    && movement.iter().all(|v| v.is_finite() && v.abs() <= 1.001)
                    && valid_angles(yaw, pitch)
                {
                    p.input = Input {
                        movement,
                        jump: jump || p.input.jump,
                        sprint,
                    };
                    p.sequence = seq;
                    p.last_input = tick;
                    p.flying = flying;
                    p.yaw = yaw;
                    p.pitch = pitch;
                }
            }
            (Channel::Control, ClientMsg::Bye) => self.net.disconnect(id),
            _ => {}
        }
    }
    pub fn tick(
        &mut self,
        stream: &mut Streamer<()>,
        tick: u64,
        dt: Duration,
    ) -> io::Result<Duration> {
        self.net.poll(dt).map_err(io::Error::other)?;
        while let Some(event) = self.net.next_event() {
            match event {
                ServerEvent::Connected(id) => {
                    self.hello_deadlines.insert(id, tick + 300);
                }
                ServerEvent::Disconnected(id, _) => {
                    self.peers.remove(&id);
                    self.hello_deadlines.remove(&id);
                }
            }
        }
        for (&id, &deadline) in &self.hello_deadlines {
            if tick >= deadline {
                self.net.disconnect(id);
            }
        }
        let mut counts = HashMap::new();
        for _ in 0..2048 {
            let Some((id, ch, bytes)) = self.net.recv() else {
                break;
            };
            let n = counts.entry(id).or_insert(0);
            *n += 1;
            if *n > 32 {
                self.net.disconnect(id);
                continue;
            }
            match decode(&bytes) {
                Ok(msg) => self.message(id, ch, msg, stream.world(), tick),
                Err(_) => self.net.disconnect(id),
            }
        }
        let now = std::time::Instant::now();
        let mut ids: Vec<_> = self.peers.keys().copied().collect();
        ids.sort_by_key(|id| id.0);
        let views: Vec<_> = ids.iter().map(|id| self.peers[id].view()).collect();
        let f = stream.tick_views(&views);
        if let Some(e) = f.errors.first() {
            return Err(io::Error::other(e.clone()));
        }
        let streaming = now.elapsed();
        let world = stream.world();
        let residents = world.chunks();
        for id in ids {
            let p = self.peers.get_mut(&id).unwrap();
            if p.player.is_none() {
                if let Some(pos) = p.arrival {
                    p.player = Player::arrival(world, pos, p.flying);
                } else if (0..8).all(|y| world.is_resident([0, y, 0])) {
                    p.player = Player::try_spawn(world);
                }
            }
            if let Some(player) = p.player.as_mut() {
                player.flying = p.flying;
                if tick.saturating_sub(p.last_input) > 15 {
                    p.input = Input::default();
                }
                player.step(world, p.input, dt.as_secs_f32());
                p.input.jump = false;
            }
            if p.ready && tick.is_multiple_of(3) {
                Self::chunks(&mut self.net, id, p, &residents, world, &self.revisions);
            }
        }
        if tick.is_multiple_of(3) {
            let mut actors: Vec<_> = self
                .peers
                .values()
                .filter_map(|p| {
                    p.player.as_ref().filter(|_| p.ready).map(|a| Actor {
                        id: p.id,
                        position: a.position,
                        yaw: p.yaw,
                        pitch: p.pitch,
                        flying: p.flying,
                    })
                })
                .collect();
            actors.sort_by_key(|p| p.id);
            let data = encode(&ServerMsg::Players {
                tick,
                players: actors,
            })?;
            for (&id, p) in &self.peers {
                if p.ready {
                    let _ = self.net.send(id, Channel::Snapshots, data.clone());
                }
            }
        }
        self.net.flush().map_err(io::Error::other)?;
        Ok(streaming)
    }
    pub fn players(&self) -> usize {
        self.peers.len()
    }
    pub fn traffic(&self) -> primal_net::TrafficStats {
        self.net.stats()
    }
    pub fn teleport(&mut self, id: u64, position: [f32; 3]) -> bool {
        if !position
            .iter()
            .all(|x| x.is_finite() && x.abs() <= 2_000_000.)
            || !(1.0..=126.0).contains(&position[1])
        {
            return false;
        }
        let Some(p) = self.peers.values_mut().find(|p| p.id == id) else {
            return false;
        };
        p.arrival = Some(position);
        p.player = None;
        p.input = Input::default();
        true
    }
    fn chunks(
        net: &mut ServerTransport,
        id: ConnectionId,
        p: &mut Peer,
        residents: &[ChunkPos],
        world: &World,
        revisions: &HashMap<ChunkPos, u64>,
    ) {
        let c = chunk_at(p.view().block);
        let mut candidates = Vec::new();
        for &k in residents {
            let dx = (k[0] - c[0]).abs();
            let dz = (k[2] - c[2]).abs();
            if dx.max(dz) <= 3 {
                candidates.push((dx * dx + dz * dz + (k[1] - c[1]).pow(2), k));
            }
        }
        let wanted: HashSet<_> = candidates.iter().map(|v| v.1).collect();
        p.sent.retain(|k, _| {
            wanted.contains(k)
                || net
                    .send(
                        id,
                        Channel::Chunks,
                        encode(&ServerMsg::Forget { chunk: *k }).unwrap(),
                    )
                    .is_err()
        });
        candidates.sort_unstable();
        let mut sent = 0;
        for (_, k) in candidates {
            let rev = revisions.get(&k).copied().unwrap_or(0);
            if p.sent.get(&k) == Some(&rev) {
                continue;
            }
            let msg = ServerMsg::Chunk {
                chunk: k,
                revision: rev,
                data: ChunkData::capture(world, k),
            };
            if net
                .send(id, Channel::Chunks, encode(&msg).unwrap())
                .is_err()
            {
                break;
            }
            p.sent.insert(k, rev);
            sent += 1;
            if sent == 12 {
                break;
            }
        }
    }
}

fn valid_angles(yaw: f32, pitch: f32) -> bool {
    yaw.is_finite()
        && yaw.abs() < 1e7
        && pitch.is_finite()
        && pitch.abs() <= std::f32::consts::FRAC_PI_2 + 0.001
}
