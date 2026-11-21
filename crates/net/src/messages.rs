use crate::Channel;
use primal_voxel::{CHUNK, ChunkPos, MAX_BLOCK, Pos, Preset, World, generate_for};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::HashMap,
    fs,
    io::{self, Write},
    path::Path,
};

pub const PROTOCOL_VERSION: u16 = 1;
const MAX_MESSAGE: usize = 32 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMsg {
    Hello {
        version: u16,
        generator: u32,
        token: [u8; 32],
    },
    Input {
        seq: u64,
        movement: [f32; 3],
        jump: bool,
        sprint: bool,
        flying: bool,
        yaw: f32,
        pitch: f32,
    },
    Edit {
        seq: u64,
        target: Pos,
        revision: u64,
        material: u8,
        yaw: f32,
        pitch: f32,
    },
    Ready,
    Bye,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Actor {
    pub id: u64,
    pub position: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub flying: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMsg {
    Welcome {
        id: u64,
        seed: u64,
    },
    Players {
        tick: u64,
        players: Vec<Actor>,
    },
    Chunk {
        chunk: ChunkPos,
        revision: u64,
        data: ChunkData,
    },
    Forget {
        chunk: ChunkPos,
    },
    Outcome {
        seq: u64,
        accepted: bool,
        reason: String,
    },
    Error(String),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChunkData {
    Sparse(Vec<(u16, u8)>),
    Full(Vec<u8>),
}

/// Protocol v1 keeps Welcome's payload identical for every preset. Both lanes are reliable.
/// Forest uses the ordered chunk lane to signal that subsequent voxel data is authoritative.
/// Older clients consume Welcome from either lane and already understand full chunk volumes.
pub fn welcome_channel(preset: Preset) -> Channel {
    match preset {
        Preset::Historical => Channel::Control,
        Preset::ForestModern => Channel::Chunks,
    }
}

pub fn encode<T: Serialize>(msg: &T) -> io::Result<Vec<u8>> {
    let bytes = postcard::to_stdvec(msg).map_err(io::Error::other)?;
    if bytes.len() > MAX_MESSAGE {
        return Err(io::Error::other("Message trop long"));
    }
    Ok(bytes)
}
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> io::Result<T> {
    if bytes.len() > MAX_MESSAGE {
        return Err(io::Error::other("Message trop long"));
    }
    let (msg, tail) = postcard::take_from_bytes(bytes).map_err(io::Error::other)?;
    if !tail.is_empty() {
        return Err(io::Error::other("Octets supplémentaires"));
    }
    Ok(msg)
}
pub fn identity(path: &Path) -> io::Result<[u8; 32]> {
    fn read(path: &Path) -> io::Result<[u8; 32]> {
        fs::read(path)?
            .try_into()
            .map_err(|_| io::Error::other("Identité locale invalide"))
    }
    match read(path) {
        Ok(token) => return Ok(token),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(e) => return Err(e),
    }
    let parent = path
        .parent()
        .filter(|x| !x.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut token = [0; 32];
    getrandom::fill(&mut token).map_err(|e| io::Error::other(e.to_string()))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(&token)?;
    temp.as_file().sync_all()?;
    match temp.persist_noclobber(path) {
        Ok(_) => Ok(token),
        Err(e) if e.error.kind() == io::ErrorKind::AlreadyExists => read(path),
        Err(e) => Err(e.error),
    }
}
pub fn direction(yaw: f32, pitch: f32) -> [f32; 3] {
    [
        yaw.sin() * pitch.cos(),
        pitch.sin(),
        yaw.cos() * pitch.cos(),
    ]
}
fn index([x, y, z]: Pos) -> u16 {
    ((x.rem_euclid(16) * 16 + y.rem_euclid(16)) * 16 + z.rem_euclid(16)) as u16
}
fn position(c: ChunkPos, i: u16) -> Pos {
    let i = i32::from(i);
    [
        c[0] * CHUNK + i / 256,
        c[1] * CHUNK + i / 16 % 16,
        c[2] * CHUNK + i % 16,
    ]
}

impl ChunkData {
    pub fn capture(world: &World, c: ChunkPos) -> Self {
        if world.preset() != Preset::Historical {
            return Self::full(world, c);
        }
        let edits = world
            .chunk_edits(c)
            .into_iter()
            .map(|(p, b)| (index(p), b))
            .collect::<Vec<_>>();
        // Postcard encodes these indices as one or two bytes plus one material byte.
        let sparse_bytes: usize = edits
            .iter()
            .map(|(i, _)| if *i < 128 { 2 } else { 3 })
            .sum();
        if sparse_bytes <= 4096 {
            Self::Sparse(edits)
        } else {
            Self::full(world, c)
        }
    }

    fn full(world: &World, c: ChunkPos) -> Self {
        Self::Full((0..4096).map(|i| world.get(position(c, i))).collect())
    }
}
pub struct Replica {
    pub world: World,
    revisions: HashMap<ChunkPos, u64>,
}
impl Replica {
    pub fn new(seed: u64) -> Self {
        Self::with_preset(seed, Preset::Historical)
    }

    pub fn with_preset(seed: u64, preset: Preset) -> Self {
        Self {
            world: World::with_preset(seed, preset),
            revisions: HashMap::new(),
        }
    }
    pub fn revision(&self, c: ChunkPos) -> Option<u64> {
        self.revisions.get(&c).copied()
    }
    /// Chunk/Forget share one reliable ordered channel, including revisits.
    pub fn apply(&mut self, msg: &ServerMsg) -> io::Result<Vec<ChunkPos>> {
        let c = match msg {
            ServerMsg::Forget { chunk } => {
                if self.revisions.remove(chunk).is_none() {
                    return Ok(vec![]);
                }
                self.world.evict_chunk(*chunk);
                *chunk
            }
            ServerMsg::Chunk {
                chunk,
                revision,
                data,
            } => {
                let c = *chunk;
                if self.revisions.get(&c).is_some_and(|r| r >= revision) {
                    return Ok(vec![]);
                }
                let base = generate_for(self.world.preset(), self.world.seed(), c)
                    .ok_or_else(|| io::Error::other("Chunk hors monde"))?;
                let mut edits = Vec::new();
                match data {
                    ChunkData::Sparse(items) => {
                        if items.len() > 4096 {
                            return Err(io::Error::other("Trop de cellules"));
                        }
                        let mut last = None;
                        for &(i, b) in items {
                            if i >= 4096 || b > MAX_BLOCK || last.is_some_and(|v| v >= i) {
                                return Err(io::Error::other("Snapshot invalide"));
                            }
                            let p = position(c, i);
                            if base.get(p) == Some(b) {
                                return Err(io::Error::other("Différence redondante"));
                            }
                            edits.push((p, b));
                            last = Some(i);
                        }
                    }
                    ChunkData::Full(cells) => {
                        if cells.len() != 4096 || cells.iter().any(|b| *b > MAX_BLOCK) {
                            return Err(io::Error::other("Volume invalide"));
                        }
                        for (i, &b) in cells.iter().enumerate() {
                            let p = position(c, i as u16);
                            if base.get(p) != Some(b) {
                                edits.push((p, b))
                            }
                        }
                    }
                }
                if self.world.is_resident(c) {
                    self.world.evict_chunk(c);
                }
                let t = self
                    .world
                    .begin_load(c)
                    .ok_or_else(|| io::Error::other("Chunk indisponible"))?;
                if !self.world.install_chunk(t, base, &edits) {
                    return Err(io::Error::other("Snapshot refusé"));
                }
                self.revisions.insert(c, *revision);
                c
            }
            _ => return Ok(vec![]),
        };
        let mut dirty = vec![c];
        for axis in 0..3 {
            for d in [-1, 1] {
                let mut n = c;
                n[axis] += d;
                if self.world.is_resident(n) {
                    dirty.push(n)
                }
            }
        }
        Ok(dirty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primal_voxel::generate_chunk;
    #[test]
    fn old_revision_cannot_replace_a_newer_surface() {
        let mut r = Replica::new(7);
        let c = [0, 7, 0];
        let newer = ServerMsg::Chunk {
            chunk: c,
            revision: 2,
            data: ChunkData::Sparse(vec![(0, 5)]),
        };
        r.apply(&newer).unwrap();
        let old = ServerMsg::Chunk {
            chunk: c,
            revision: 1,
            data: ChunkData::Sparse(vec![]),
        };
        assert!(r.apply(&old).unwrap().is_empty());
        assert_eq!(r.world.lookup([0, 112, 0]), Some(5));
    }
    #[test]
    fn dense_and_sparse_snapshots_round_trip_and_select_smaller_wire_format() {
        let c = [0, 7, 0];
        let mut world = World::streamed(19);
        let t = world.begin_load(c).unwrap();
        world.install_chunk(t, generate_chunk(19, c).unwrap(), &[]);
        for n in [0u16, 16, 2048] {
            for i in 0..n {
                world.set(position(c, i), 7);
            }
            let choice = ChunkData::capture(&world, c);
            assert_eq!(matches!(choice, ChunkData::Full(_)), n == 2048);
            let msg = ServerMsg::Chunk {
                chunk: c,
                revision: u64::from(n),
                data: choice,
            };
            let mut rep = Replica::new(19);
            rep.apply(&decode(&encode(&msg).unwrap()).unwrap()).unwrap();
            assert_eq!(world.chunk_edits(c), rep.world.chunk_edits(c));
        }
    }

    #[test]
    fn forest_chunk_requires_no_local_forest_generator() {
        let seed = 19;
        let c = [0, 1, 0];
        let mut world = World::with_preset(seed, Preset::ForestModern);
        let ticket = world.begin_load(c).unwrap();
        assert!(world.install_chunk(
            ticket,
            generate_for(Preset::ForestModern, seed, c).unwrap(),
            &[],
        ));
        // Include non-generated geometry and ensure the footprint also differs naturally.
        let edited = position(c, 0);
        assert!(world.set(edited, if world.get(edited) == 7 { 0 } else { 7 }));
        let historical = generate_for(Preset::Historical, seed, c).unwrap();
        assert!((1..4096).any(|i| {
            let p = position(c, i);
            historical.get(p) != world.lookup(p)
        }));
        let data = ChunkData::capture(&world, c);
        assert!(matches!(data, ChunkData::Full(_)));
        let msg = ServerMsg::Chunk {
            chunk: c,
            revision: 0,
            data,
        };
        let msg = decode(&encode(&msg).unwrap()).unwrap();
        for preset in [Preset::Historical, Preset::ForestModern] {
            let mut replica = Replica::with_preset(seed, preset);
            replica.apply(&msg).unwrap();
            for i in 0..4096 {
                let p = position(c, i);
                assert_eq!(replica.world.lookup(p), world.lookup(p), "{p:?}");
            }
        }
    }
}
