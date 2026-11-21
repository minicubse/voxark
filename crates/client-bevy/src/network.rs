use super::{
    CameraRig, Options, Terrain,
    terrain::{self, Geometry},
};
use bevy::{math::DVec3, prelude::*, window::CursorOptions};
use primal_net::{Actor, ClientMesher, ClientMsg, Replica, ServerMsg, link::Link};
use primal_voxel::{ChunkPos, chunk_at};
use std::{
    collections::HashMap,
    sync::{Mutex, mpsc},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[derive(Resource)]
pub struct Online(pub Mutex<Session>);
pub struct Session {
    style: super::forest::Style,
    lit: bool,
    address: String,
    input_hz: u32,
    token: [u8; 32],
    link: Option<Link>,
    retry: Instant,
    replica: Option<Replica>,
    mesher: ClientMesher<Geometry>,
    id: Option<u64>,
    own: Option<Actor>,
    peers: HashMap<u64, Actor>,
    avatars: HashMap<u64, Entity>,
    tick: Option<u64>,
    avatar: Option<(Handle<Mesh>, Handle<StandardMaterial>)>,
    captured: bool,
    flying: bool,
    selected: u8,
    seq: u64,
    pub player_count: usize,
    driver: Option<primal_net::playtest::Playtest>,
}
fn mesher() -> ClientMesher<Geometry> {
    ClientMesher::new(|s| terrain::prepare(&s.greedy_mesh_local()).0)
}
impl Session {
    pub fn new(
        addr: String,
        token: [u8; 32],
        input_hz: u32,
        playtest: bool,
        style: super::forest::Style,
        lit: bool,
    ) -> Self {
        Self {
            style,
            lit,
            address: addr,
            input_hz,
            token,
            link: None,
            retry: Instant::now(),
            replica: None,
            mesher: mesher(),
            id: None,
            own: None,
            peers: HashMap::new(),
            avatars: HashMap::new(),
            tick: None,
            avatar: None,
            captured: false,
            flying: false,
            selected: 1,
            seq: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64,
            player_count: 0,
            driver: playtest.then(primal_net::playtest::Playtest::open),
        }
    }
    pub fn inputs(&self) -> u64 {
        self.link.as_ref().map_or(0, |l| {
            l.sent_inputs.load(std::sync::atomic::Ordering::Relaxed)
        })
    }
    pub fn ready(&self) -> bool {
        self.own.is_some()
            && self
                .replica
                .as_ref()
                .is_some_and(|r| r.world.resident_len() >= 392)
            && self.mesher.pending() == 0
    }
    fn reset(&mut self, terrain: &mut Terrain, cmds: &mut Commands) {
        self.link.take();
        self.id = None;
        self.own = None;
        self.tick = None;
        self.player_count = 0;
        self.replica = None;
        self.mesher = mesher();
        terrain.clear();
        self.peers.clear();
        for (_, e) in self.avatars.drain() {
            cmds.entity(e).despawn();
        }
        self.retry = Instant::now() + Duration::from_secs(1);
    }
    fn receive(&mut self, msg: ServerMsg, rig: &mut CameraRig, bench: bool) -> Result<(), String> {
        match &msg {
            ServerMsg::Welcome { id, seed } => {
                if self.link.as_ref().is_some_and(|l| l.forest_modern()) {
                    let seed = *seed;
                    let style = self.style;
                    let lit = self.lit;
                    self.mesher =
                        ClientMesher::new(move |s| super::forest::prepare(s, style, lit, seed).0);
                }
                self.replica = Some(Replica::with_preset(
                    *seed,
                    if self.link.as_ref().is_some_and(|l| l.forest_modern()) {
                        primal_voxel::Preset::ForestModern
                    } else {
                        primal_voxel::Preset::Historical
                    },
                ));
                self.id = Some(*id);
                println!("Connexion joueur {id}, seed {seed}");
            }
            ServerMsg::Players { tick, players } => {
                if self.id.is_none() || self.tick.is_some_and(|old| old >= *tick) {
                    return Ok(());
                }
                self.tick = Some(*tick);
                self.player_count = players.len();
                self.peers.clear();
                for actor in players {
                    if Some(actor.id) == self.id {
                        if self.own.is_none() {
                            self.flying = actor.flying;
                            if !bench {
                                rig.yaw = actor.yaw;
                                rig.pitch = actor.pitch;
                            }
                        }
                        self.own = Some(actor.clone());
                    } else {
                        self.peers.insert(actor.id, actor.clone());
                    }
                }
            }
            ServerMsg::Error(e) => return Err(e.clone()),
            ServerMsg::Outcome {
                seq,
                accepted,
                reason,
            } => println!("Édition {seq} acceptée={accepted} {reason}"),
            _ => {}
        }
        if let Some(r) = self.replica.as_mut() {
            self.mesher.dirty(r.apply(&msg).map_err(|e| e.to_string())?);
        }
        Ok(())
    }
    fn pump(&mut self, rig: &mut CameraRig, bench: bool) -> Result<(), String> {
        for _ in 0..64 {
            match self.link.as_ref().unwrap().events.try_recv() {
                Ok(m) => self.receive(m, rig, bench)?,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return Err("Connexion terminée".into()),
            }
        }
        Ok(())
    }
}
#[allow(clippy::too_many_arguments)]
pub fn update(
    online: Option<ResMut<Online>>,
    o: Res<Options>,
    mut rig: ResMut<CameraRig>,
    mut terrain: ResMut<Terrain>,
    mut cmds: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    cursor: Single<&CursorOptions>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut online) = online else { return };
    let n = online.0.get_mut().unwrap();
    if keys.just_pressed(KeyCode::KeyR) {
        n.reset(&mut terrain, &mut cmds);
    }
    if n.link.is_none() {
        if Instant::now() < n.retry {
            return;
        }
        match Link::open(&n.address, n.token, n.input_hz) {
            Ok(l) => n.link = Some(l),
            Err(e) => {
                eprintln!("Connexion : {e}");
                n.retry = Instant::now() + Duration::from_secs(2);
                return;
            }
        }
    }
    if let Err(e) = n.pump(&mut rig, o.frames > 0) {
        eprintln!("{e}");
        n.reset(&mut terrain, &mut cmds);
        return;
    }
    let Some(own) = n.own.as_ref() else { return };
    let pos = own.position;
    let Some(replica) = &n.replica else { return };
    let updates = n.mesher.tick(replica, pos);
    for (c, payload) in updates {
        match payload {
            Some(p) => terrain.install(c, p),
            None => terrain.remove(c),
        }
    }
    if o.frames == 0 {
        rig.eye = DVec3::from_array(pos.map(f64::from)) + DVec3::Y * 1.62;
    }
    let mut click = None;
    if let Some(driver) = n.driver.as_mut() {
        use primal_net::playtest::Action;
        match driver.next_action() {
            Some(Action::Look(y, p)) => {
                rig.yaw = y;
                rig.pitch = p;
            }
            Some(Action::Click(v)) => {
                click = Some(v);
                if v > 0 {
                    n.selected = v;
                }
            }
            Some(Action::Fly) => n.flying = true,
            Some(Action::Photo) => {
                if let Some(path) = &o.capture {
                    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
                    cmds.spawn(Screenshot::primary_window())
                        .observe(save_to_disk(path.clone()));
                }
            }
            Some(Action::Quit) => {
                exit.write(AppExit::Success);
            }
            _ => {}
        }
        if let Some(p) = driver.watch() {
            driver.observe(
                replica,
                n.id.unwrap(),
                n.peers.len(),
                n.mesher.pending() == 0 && terrain.idle() && terrain.has_geometry(chunk_at(p)),
            );
        }
    }
    let captured = !cursor.visible && o.frames == 0;
    let f = Vec3::new(rig.yaw.sin(), 0., rig.yaw.cos());
    let axis = |a, b| u8::from(keys.pressed(a)) as f32 - u8::from(keys.pressed(b)) as f32;
    if captured && keys.just_pressed(KeyCode::KeyF) {
        n.flying = !n.flying;
    }
    let movement = if captured {
        (f * axis(KeyCode::KeyW, KeyCode::KeyS)
            + f.cross(Vec3::Y) * axis(KeyCode::KeyD, KeyCode::KeyA)
            + Vec3::Y
                * if n.flying {
                    axis(KeyCode::Space, KeyCode::ControlLeft)
                } else {
                    0.
                })
        .clamp_length_max(1.)
    } else {
        Vec3::ZERO
    };
    for (i, key) in [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
    ]
    .into_iter()
    .enumerate()
    {
        if keys.just_pressed(key) {
            n.selected = i as u8 + 1;
        }
    }
    let link = n.link.as_ref().unwrap();
    {
        let mut ctrl = link.input.lock().unwrap();
        ctrl.ready = true;
        ctrl.movement = movement.to_array();
        ctrl.flying = n.flying;
        ctrl.sprint = captured && keys.pressed(KeyCode::ShiftLeft);
        ctrl.jump |= captured && keys.just_pressed(KeyCode::Space);
        ctrl.yaw = rig.yaw;
        ctrl.pitch = rig.pitch;
    }
    let remove = click == Some(0) || buttons.just_pressed(MouseButton::Left);
    let place = click.is_some_and(|v| v > 0) || buttons.just_pressed(MouseButton::Right);
    if (click.is_some() || captured && n.captured)
        && (remove || place)
        && let Some(hit) = primal_simulation::raycast(
            &replica.world,
            rig.eye.as_vec3().to_array(),
            rig.front().to_array(),
            primal_simulation::MAX_REACH,
        )
    {
        let target = if remove { hit.block } else { hit.adjacent };
        if let Some(revision) = replica.revision(chunk_at(target)) {
            n.seq += 1;
            if let Err(e) = link.edit(ClientMsg::Edit {
                seq: n.seq,
                target,
                revision,
                material: if remove { 0 } else { n.selected },
                yaw: rig.yaw,
                pitch: rig.pitch,
            }) {
                eprintln!("Édition : {e}");
            }
        }
    }
    n.captured = captured;
    let origin: ChunkPos = if (rig.eye
        - DVec3::from_array(terrain.origin.map(|x| f64::from(x) * 16.)))
    .abs()
    .max_element()
        > 128.
    {
        terrain::origin_for(rig.eye)
    } else {
        terrain.origin
    };
    let rel = DVec3::from_array(origin.map(|a| f64::from(a) * 16.));
    let (mesh, mat) = n.avatar.get_or_insert_with(|| {
        (
            meshes.add(Cuboid::new(0.6, 1.6, 0.6)),
            materials.add(StandardMaterial {
                unlit: true,
                base_color: Color::srgb_u8(201, 135, 64),
                ..default()
            }),
        )
    });
    n.avatars.retain(|id, e| {
        let keep = n.peers.contains_key(id);
        if !keep {
            cmds.entity(*e).despawn();
        }
        keep
    });
    for (id, a) in &n.peers {
        let transform = Transform::from_translation(
            (DVec3::from_array(a.position.map(f64::from)) + DVec3::Y * 0.8 - rel).as_vec3(),
        )
        .with_rotation(Quat::from_rotation_y(a.yaw));
        if let Some(e) = n.avatars.get(id) {
            cmds.entity(*e).insert(transform);
        } else {
            let e = cmds
                .spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), transform))
                .id();
            n.avatars.insert(*id, e);
        }
    }
}
