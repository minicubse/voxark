use crate::disk::Disk;
use crate::metrics::TickTimes;
use primal_ledger::{PlayerState, PlayerUpdate};
use primal_net::{Actor, direction};
use primal_net::{
    CREATURE_PROTOCOL_VERSION, Channel, ChunkData, ClientMsg, ConnectionId, CreatureTraffic,
    PROTOCOL_VERSION, ServerEvent, ServerMsg, ServerTransport, decode, encode, welcome_channel,
};
use primal_simulation::creatures::{CreatureState, CreatureWorld};
use primal_simulation::{Input, Player};
use primal_streaming::{Streamer, View};
use primal_voxel::{ChunkPos, GENERATOR_VERSION, World, chunk_at, generate_for};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::{
    collections::{HashMap, HashSet},
    io,
    time::{Duration, Instant},
};

pub const SNAPSHOT_TICKS: u64 = 3;
const INTEREST_CHUNKS: i32 = 3;

struct EditRequest {
    seq: u64,
    target: [i32; 3],
    revision: u64,
    material: u8,
    yaw: f32,
    pitch: f32,
}
struct Peer {
    id: u64,
    token: [u8; 32],
    ready: bool,
    wants_creatures: bool,
    player: Option<Player>,
    sent: HashMap<ChunkPos, u64>,
    arrival: Option<[f32; 3]>,
    input: Input,
    last_input: u64,
    sequence: u64,
    edit_sequence: u64,
    next_edit: u64,
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
struct Admission {
    token: [u8; 32],
    wants_creatures: bool,
    result: Receiver<io::Result<PlayerState>>,
}
struct Pending {
    token: [u8; 32],
    seq: u64,
    target: [i32; 3],
    block: u8,
}
pub struct Room {
    net: ServerTransport,
    peers: HashMap<ConnectionId, Peer>,
    disk: Disk,
    admissions: HashMap<ConnectionId, Admission>,
    pending: HashMap<ChunkPos, Pending>,
    profiles: HashMap<[u8; 32], PlayerState>,
    unsaved: HashMap<[u8; 32], PlayerState>,
    hello_deadlines: HashMap<ConnectionId, u64>,
    revisions: HashMap<ChunkPos, u64>,
    creatures: CreatureWorld,
    creature_states: Vec<CreatureState>,
    creature_traffic: CreatureTraffic,
    tick_hz: u32,
}
impl Room {
    pub fn new(
        net: ServerTransport,
        disk: Disk,
        revisions: HashMap<ChunkPos, u64>,
        profiles: HashMap<[u8; 32], PlayerState>,
        creatures: CreatureWorld,
        tick_hz: u32,
    ) -> Self {
        Self {
            net,
            peers: HashMap::new(),
            disk,
            admissions: HashMap::new(),
            pending: HashMap::new(),
            profiles,
            unsaved: HashMap::new(),
            hello_deadlines: HashMap::new(),
            revisions,
            creatures,
            creature_states: Vec::new(),
            creature_traffic: CreatureTraffic::default(),
            tick_hz,
        }
    }
    fn message(
        &mut self,
        id: ConnectionId,
        channel: Channel,
        msg: ClientMsg,
        world: &mut World,
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
            ) if !self.peers.contains_key(&id) && !self.admissions.contains_key(&id) => {
                if !matches!(version, PROTOCOL_VERSION | CREATURE_PROTOCOL_VERSION)
                    || generator != GENERATOR_VERSION
                    || token == [0; 32]
                    || self.peers.values().any(|p| p.token == token)
                    || self.admissions.values().any(|p| p.token == token)
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
                let initial = PlayerState {
                    id: 1,
                    position: [0.5, 16., 0.5],
                    yaw: -0.35,
                    pitch: -0.12,
                    flying: false,
                };
                match self.disk.register(token, initial) {
                    Ok(result) => {
                        self.admissions.insert(
                            id,
                            Admission {
                                token,
                                wants_creatures: version == CREATURE_PROTOCOL_VERSION,
                                result,
                            },
                        );
                    }
                    Err(_) => {
                        let _ = self.net.send(
                            id,
                            Channel::Control,
                            encode(&ServerMsg::Error("Stockage occupé, réessayez".into())).unwrap(),
                        );
                    }
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
            (
                Channel::Edits,
                ClientMsg::Edit {
                    seq,
                    target,
                    revision,
                    material,
                    yaw,
                    pitch,
                },
            ) => {
                let request = EditRequest {
                    seq,
                    target,
                    revision,
                    material,
                    yaw,
                    pitch,
                };
                let result = self.edit(id, request, world, tick);
                if let Err(reason) = result {
                    self.outcome(id, seq, false, reason);
                }
            }
            (Channel::Control, ClientMsg::Bye) => self.net.disconnect(id),
            _ => {}
        }
    }
    fn edit(
        &mut self,
        id: ConnectionId,
        req: EditRequest,
        world: &mut World,
        tick: u64,
    ) -> Result<(), &'static str> {
        let p = self.peers.get_mut(&id).ok_or("Connexion requise")?;
        if !p.ready || req.seq <= p.edit_sequence {
            return Err("Commande ancienne");
        }
        p.edit_sequence = req.seq;
        if tick < p.next_edit {
            return Err("Interactions trop rapides");
        }
        p.next_edit = tick + 2;
        let c = chunk_at(req.target);
        if p.sent.get(&c) != Some(&req.revision)
            || self.revisions.get(&c).copied().unwrap_or(0) != req.revision
        {
            return Err("Révision périmée");
        }
        if !valid_angles(req.yaw, req.pitch) {
            return Err("Orientation invalide");
        }
        let actor = p.player.as_ref().ok_or("Apparition en cours")?;
        let action = if req.material == 0 {
            primal_simulation::Edit::Remove
        } else {
            primal_simulation::Edit::Place(req.material)
        };
        let (target, block) =
            primal_simulation::resolve_edit(world, actor, direction(req.yaw, req.pitch), action)
                .map_err(|_| "Interaction hors portée ou interdite")?;
        if target != req.target {
            return Err("La cellule visée a changé");
        }
        if block != 0
            && self
                .peers
                .values()
                .any(|p| p.player.as_ref().is_some_and(|a| a.overlaps_cell(target)))
        {
            return Err("Joueur dans le bloc");
        }
        if self.pending.contains_key(&c) {
            return Err("Transaction déjà en cours");
        }
        let token = self.peers[&id].token;
        let base = generate_for(world.preset(), world.seed(), c)
            .and_then(|g| g.get(target))
            .ok_or("Chunk indisponible")?;
        let mut edits = world.chunk_edits(c);
        edits.retain(|(p, _)| *p != target);
        if base != block {
            edits.push((target, block));
            edits.sort_unstable_by_key(|v| v.0);
        }
        self.disk
            .submit_edit(token, req.seq, c, req.revision, edits)
            .map_err(|_| "Sauvegarde occupée")?;
        self.pending.insert(
            c,
            Pending {
                token,
                seq: req.seq,
                target,
                block,
            },
        );
        println!(
            "{}",
            serde_json::json!({"event":"save_queued","seq":req.seq,"chunk":c})
        );
        Ok(())
    }
    fn outcome(&mut self, id: ConnectionId, seq: u64, accepted: bool, reason: &str) {
        let msg = ServerMsg::Outcome {
            seq,
            accepted,
            reason: reason.into(),
        };
        if self
            .net
            .send(id, Channel::Control, encode(&msg).unwrap())
            .is_err()
        {
            self.net.disconnect(id);
        }
    }
    fn poll_disk(&mut self, world: &mut World) -> io::Result<()> {
        while let Some(done) = self.disk.poll() {
            let done = done?;
            let pending = self
                .pending
                .remove(&done.chunk)
                .ok_or_else(|| io::Error::other("Accusé sans transaction"))?;
            if pending.token != done.token || pending.seq != done.seq {
                return Err(io::Error::other("Identité de transaction incohérente"));
            }
            if !done.rejected && !done.duplicate {
                if !world.set(pending.target, pending.block) {
                    return Err(io::Error::other("Chunk épinglé incohérent après commit"));
                }
                world.mark_saved(done.chunk, world.chunk_revision(done.chunk).unwrap());
                self.revisions.insert(done.chunk, done.revision);
            }
            if let Some(id) = self
                .peers
                .iter()
                .find_map(|(&id, p)| (p.token == done.token).then_some(id))
            {
                self.outcome(
                    id,
                    done.seq,
                    !done.rejected,
                    if done.rejected {
                        "Identifiant déjà utilisé ou révision périmée"
                    } else {
                        ""
                    },
                );
            }
        }
        let mut completed = vec![];
        for (&id, a) in &self.admissions {
            match a.result.try_recv() {
                Ok(result) => completed.push((id, result?)),
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => {
                    return Err(io::Error::other("Inscription interrompue"));
                }
            }
        }
        for (id, stored) in completed {
            let Admission {
                token,
                wants_creatures,
                ..
            } = self.admissions.remove(&id).unwrap();
            let previous = self.profiles.get(&token);
            let resumed = previous.is_some();
            let state = previous.cloned().unwrap_or(stored);
            if self
                .net
                .send(
                    id,
                    welcome_channel(world.preset()),
                    encode(&ServerMsg::Welcome {
                        id: state.id,
                        seed: world.seed(),
                    })?,
                )
                .is_err()
            {
                continue;
            }
            self.hello_deadlines.remove(&id);
            self.peers.insert(
                id,
                Peer {
                    id: state.id,
                    token,
                    ready: false,
                    wants_creatures,
                    player: None,
                    sent: HashMap::new(),
                    arrival: resumed.then_some(state.position),
                    input: Input::default(),
                    last_input: 0,
                    sequence: 0,
                    edit_sequence: 0,
                    next_edit: 0,
                    yaw: state.yaw,
                    pitch: state.pitch,
                    flying: state.flying,
                },
            );
            self.profiles.insert(token, state);
        }
        Ok(())
    }
    fn remember(&mut self, id: ConnectionId) {
        let Some(p) = self.peers.get(&id) else { return };
        let Some(pos) = p.player.as_ref().map(|p| p.position).or(p.arrival) else {
            return;
        };
        let state = PlayerState {
            id: p.id,
            position: pos,
            yaw: p.yaw,
            pitch: p.pitch,
            flying: p.flying,
        };
        if self.profiles.get(&p.token) != Some(&state) {
            self.profiles.insert(p.token, state.clone());
            self.unsaved.insert(p.token, state);
        }
    }
    fn checkpoints(&mut self) -> io::Result<()> {
        if self.unsaved.is_empty() {
            return Ok(());
        }
        let states = self
            .unsaved
            .iter()
            .map(|(&token, s)| PlayerUpdate {
                token,
                state: s.clone(),
            })
            .collect();
        match self.disk.save_players(states) {
            Ok(()) => {
                self.unsaved.clear();
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(()),
            Err(e) => Err(e),
        }
    }
    pub fn finish(mut self, world: &mut World) -> io::Result<()> {
        for id in self.peers.keys().copied().collect::<Vec<_>>() {
            self.remember(id);
        }
        let start = Instant::now();
        while !self.unsaved.is_empty() || !self.pending.is_empty() {
            self.poll_disk(world)?;
            self.checkpoints()?;
            if start.elapsed() > Duration::from_secs(30) {
                return Err(io::Error::other("Délai de vidage du disque"));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        self.disk.finish()
    }
    pub fn tick(
        &mut self,
        stream: &mut Streamer<()>,
        tick: u64,
        dt: Duration,
    ) -> io::Result<TickTimes> {
        self.poll_disk(stream.world_mut())?;
        self.net.poll(dt).map_err(io::Error::other)?;
        while let Some(event) = self.net.next_event() {
            match event {
                ServerEvent::Connected(id) => {
                    self.hello_deadlines.insert(id, tick + 300);
                }
                ServerEvent::Disconnected(id, _) => {
                    self.remember(id);
                    self.admissions.remove(&id);
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
                Ok(msg) => self.message(id, ch, msg, stream.world_mut(), tick),
                Err(_) => self.net.disconnect(id),
            }
        }
        let now = Instant::now();
        let mut ids: Vec<_> = self.peers.keys().copied().collect();
        ids.sort_by_key(|id| id.0);
        let views: Vec<_> = ids.iter().map(|id| self.peers[id].view()).collect();
        let pins: Vec<_> = self.pending.keys().copied().collect();
        let f = stream.tick_views_pinned(&views, &pins);
        if let Some(e) = f.errors.first() {
            return Err(io::Error::other(e.clone()));
        }
        let streaming = now.elapsed();
        let now = Instant::now();
        self.creature_states.clear();
        self.creature_states.extend(self.creatures.states(tick));
        let creature_simulation = now.elapsed();
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
                let old = player.clone();
                player.step(world, p.input, dt.as_secs_f32());
                if self
                    .pending
                    .values()
                    .any(|e| e.block != 0 && player.overlaps_cell(e.target))
                {
                    *player = old;
                }

                p.input.jump = false;
            }
            if p.ready && tick.is_multiple_of(SNAPSHOT_TICKS) {
                Self::chunks(&mut self.net, id, p, &residents, world, &self.revisions);
            }
        }
        if tick.is_multiple_of(SNAPSHOT_TICKS) {
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
                    if p.wants_creatures {
                        let center = chunk_at(p.view().block);
                        let creatures = self
                            .creature_states
                            .iter()
                            .filter(|state| {
                                let chunk = chunk_at(state.position.map(|v| v.floor() as i32));
                                (chunk[0] - center[0])
                                    .abs()
                                    .max((chunk[2] - center[2]).abs())
                                    <= INTEREST_CHUNKS
                            })
                            .copied()
                            .collect();
                        let bytes = encode(&ServerMsg::Creatures {
                            tick,
                            tick_hz: self.tick_hz,
                            creatures,
                        })?;
                        let len = bytes.len();
                        if self.net.send(id, Channel::Snapshots, bytes).is_ok() {
                            self.creature_traffic.record(len);
                        }
                    }
                }
            }
        }
        if tick.is_multiple_of(60) {
            for id in self.peers.keys().copied().collect::<Vec<_>>() {
                self.remember(id);
            }
        }
        self.checkpoints()?;
        self.net.flush().map_err(io::Error::other)?;
        Ok(TickTimes {
            streaming,
            creature_simulation,
        })
    }
    pub fn players(&self) -> usize {
        self.peers.len()
    }
    pub fn traffic(&self) -> primal_net::TrafficStats {
        self.net.stats()
    }
    pub fn creature_traffic(&self) -> CreatureTraffic {
        self.creature_traffic
    }
    pub fn spawn_creature(&mut self) -> Option<u64> {
        self.creatures.spawn().map(|id| id.0)
    }
    pub fn despawn_creature(&mut self, id: u64) -> bool {
        self.creatures
            .despawn(primal_simulation::creatures::CreatureId(id))
    }
    pub fn creatures_at(&self, tick: u64) -> impl Iterator<Item = CreatureState> + '_ {
        self.creatures.states(tick)
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
            if dx.max(dz) <= INTEREST_CHUNKS {
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
