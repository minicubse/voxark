use super::{CHECKSUM, GENERATOR, MAX_FILE, WorldLock, decode, directory_for, invalid};
use primal_voxel::{CHUNK, ChunkPos, MAX_BLOCK, Pos, World};
use std::collections::{BTreeMap, hash_map::DefaultHasher};
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{self, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tempfile::{Builder, NamedTempFile};

const VERSION: u16 = 2;
const MANIFEST: &str = "world.pvxd";
const MANIFEST_BYTES: usize = 16 + CHECKSUM;
const REGION_HEADER: usize = 32;
const CHUNKS_PER_EDGE: i32 = 4;
const CHUNK_CELLS: u32 = (CHUNK * CHUNK * CHUNK) as u32;
const REGION_CELLS: u32 = CHUNK_CELLS * (CHUNKS_PER_EDGE.pow(3) as u32);
const MAX_REGION_BYTES: usize = REGION_HEADER + REGION_CELLS as usize * 4 + CHECKSUM;
const LOCK_COUNT: usize = 32;

/// A clone shares the session lock and regional transactions with its original handle.
/// Blocking I/O belongs on workers. Callers order snapshots of the same chunk by revision.
#[derive(Clone)]
pub struct RegionStore(Arc<Session>);

struct Session {
    directory: PathBuf,
    seed: u64,
    _world_lock: WorldLock,
    // Fixed stripes bound bookkeeping irrespective of the number of regions visited.
    locks: [RwLock<()>; LOCK_COUNT],
}

impl RegionStore {
    /// `path` is the original --world path. A v1 source remains available byte-for-byte.
    pub fn open(path: &Path, default_seed: u64) -> io::Result<Self> {
        let world_lock = WorldLock::acquire(path)?;
        let directory = Self::directory(path);
        let seed = if directory.try_exists()? {
            read_manifest(&directory)?
        } else {
            initialize(path, &directory, default_seed)?
        };
        Ok(Self(Arc::new(Session {
            directory,
            seed,
            _world_lock: world_lock,
            locks: std::array::from_fn(|_| RwLock::new(())),
        })))
    }

    /// The complete v2 save directory adjacent to the legacy filename.
    pub fn directory(path: &Path) -> PathBuf {
        let mut name = path.as_os_str().to_owned();
        name.push(".regions");
        name.into()
    }

    pub fn seed(&self) -> u64 {
        self.0.seed
    }

    /// Returns absolute positions, sorted lexicographically. An untouched chunk has no deltas.
    pub fn load_chunk(&self, chunk: ChunkPos) -> io::Result<Vec<(Pos, u8)>> {
        let location = Location::new(chunk)?;
        let _guard = self
            .region_lock(location.region)
            .read()
            .map_err(lock_error)?;
        let records = self.read_region(location.region)?;
        Ok(records[location.range(&records)]
            .iter()
            .map(|&record| location.unpack(record))
            .collect())
    }

    /// Atomically replaces this chunk's complete normalized delta snapshot.
    /// An empty slice clears the chunk. Other chunks in its region remain intact.
    pub fn save_chunk(&self, chunk: ChunkPos, edits: &[(Pos, u8)]) -> io::Result<()> {
        let location = Location::new(chunk)?;
        if edits.len() > CHUNK_CELLS as usize {
            return Err(input("Trop de modifications dans le chunk"));
        }
        let mut replacement = Vec::with_capacity(edits.len());
        for &(position, block) in edits {
            if block > MAX_BLOCK || chunk_of(position) != chunk {
                return Err(input("Position ou matériau invalide pour le chunk"));
            }
            replacement.push(location.pack(position, block));
        }
        replacement.sort_unstable();
        if replacement.windows(2).any(|p| cell(p[0]) == cell(p[1])) {
            return Err(input("Position dupliquée dans le chunk"));
        }
        let region = location.region;
        let _guard = self.region_lock(region).write().map_err(lock_error)?;
        let mut records = self.read_region(region)?;
        let range = location.range(&records);
        if records[range.clone()] == replacement {
            return Ok(());
        }
        records.splice(range, replacement);
        let path = region_path(&self.0.directory, region);
        if records.is_empty() {
            fs::remove_file(path)?;
            sync_directory(&self.0.directory)
        } else {
            commit_file(&path, &region_bytes(self.seed(), region, &records))
        }
    }

    fn read_region(&self, region: ChunkPos) -> io::Result<Vec<u32>> {
        let bytes = match File::open(region_path(&self.0.directory, region)) {
            Ok(file) => read_bounded(file, MAX_REGION_BYTES)?,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                // A disappeared save directory is an I/O failure even for an untouched chunk.
                fs::metadata(&self.0.directory)?;
                return Ok(Vec::new());
            }
            Err(err) => return Err(err),
        };
        let payload = checked_payload(&bytes, b"PVRG", REGION_HEADER)?;
        if payload[8..16] != self.seed().to_le_bytes() {
            return Err(invalid("Seed de région incompatible"));
        }
        for (offset, coordinate) in payload[16..28].as_chunks::<4>().0.iter().zip(region) {
            if i32::from_le_bytes(*offset) != coordinate {
                return Err(invalid("Coordonnées de région incompatibles"));
            }
        }
        let count = u32::from_le_bytes(payload[28..32].try_into().unwrap());
        let (words, remainder) = payload[REGION_HEADER..].as_chunks::<4>();
        if count > REGION_CELLS || !remainder.is_empty() || words.len() != count as usize {
            return Err(invalid("Nombre de modifications de région incohérent"));
        }
        let mut records = Vec::with_capacity(words.len());
        let mut previous = None;
        for &word in words {
            let record = u32::from_le_bytes(word);
            let index = cell(record);
            if index >= REGION_CELLS
                || record as u8 > MAX_BLOCK
                || previous.is_some_and(|p| p >= index)
            {
                return Err(invalid(
                    "Modification de région invalide, dupliquée ou désordonnée",
                ));
            }
            previous = Some(index);
            records.push(record);
        }
        Ok(records)
    }

    fn region_lock(&self, region: ChunkPos) -> &RwLock<()> {
        let mut hash = DefaultHasher::new();
        region.hash(&mut hash);
        &self.0.locks[hash.finish() as usize % LOCK_COUNT]
    }
}

fn lock_error<T>(_: std::sync::PoisonError<T>) -> io::Error {
    io::Error::other("Transaction régionale interrompue")
}

fn input(message: &'static str) -> io::Error {
    io::Error::new(ErrorKind::InvalidInput, message)
}

fn chunk_of(position: Pos) -> ChunkPos {
    position.map(|axis| axis.div_euclid(CHUNK))
}

// Record order groups all edits of a chunk into one contiguous interval.
struct Location {
    region: ChunkPos,
    origin: Pos,
    first_cell: u32,
}

impl Location {
    fn new(chunk: ChunkPos) -> io::Result<Self> {
        let mut origin = [0; 3];
        for (axis, value) in chunk.into_iter().enumerate() {
            origin[axis] = value
                .checked_mul(CHUNK)
                .ok_or_else(|| input("Chunk hors de l'espace i32"))?;
        }
        let region = chunk.map(|v| v.div_euclid(CHUNKS_PER_EDGE));
        let local = chunk.map(|v| v.rem_euclid(CHUNKS_PER_EDGE));
        Ok(Self {
            region,
            origin,
            first_cell: linear(local, CHUNKS_PER_EDGE) * CHUNK_CELLS,
        })
    }

    fn range(&self, records: &[u32]) -> std::ops::Range<usize> {
        let start = records.partition_point(|&r| cell(r) < self.first_cell);
        let end = records.partition_point(|&r| cell(r) < self.first_cell + CHUNK_CELLS);
        start..end
    }

    fn pack(&self, pos: Pos, block: u8) -> u32 {
        let local = pos.map(|axis| axis.rem_euclid(CHUNK));
        ((self.first_cell + linear(local, CHUNK)) << 8) | u32::from(block)
    }

    fn unpack(&self, record: u32) -> (Pos, u8) {
        let i = (cell(record) - self.first_cell) as i32;
        (
            [
                self.origin[0] + i / (CHUNK * CHUNK),
                self.origin[1] + i / CHUNK % CHUNK,
                self.origin[2] + i % CHUNK,
            ],
            record as u8,
        )
    }
}

fn linear([x, y, z]: Pos, edge: i32) -> u32 {
    ((x * edge + y) * edge + z) as u32
}

fn cell(record: u32) -> u32 {
    record >> 8
}

fn region_path(directory: &Path, [x, y, z]: ChunkPos) -> PathBuf {
    directory.join(format!("r.{x}.{y}.{z}.pvr"))
}

fn read_bounded(file: File, limit: usize) -> io::Result<Vec<u8>> {
    let size = file.metadata()?.len();
    if size > limit as u64 {
        return Err(invalid("Fichier de sauvegarde trop grand"));
    }
    let mut data = Vec::with_capacity(size as usize);
    file.take(limit as u64 + 1).read_to_end(&mut data)?;
    if data.len() > limit {
        return Err(invalid("Fichier de sauvegarde trop grand"));
    }
    Ok(data)
}

fn checked_payload<'a>(bytes: &'a [u8], magic: &[u8; 4], header: usize) -> io::Result<&'a [u8]> {
    if bytes.len() < header + CHECKSUM || &bytes[..4] != magic {
        return Err(invalid("Signature ou taille PVXD v2 invalide"));
    }
    let (payload, crc) = bytes.split_at(bytes.len() - CHECKSUM);
    if crc32fast::hash(payload).to_le_bytes() != crc {
        return Err(invalid("Somme de contrôle PVXD v2 incorrecte"));
    }
    if payload[4..6] != VERSION.to_le_bytes() || payload[6..8] != GENERATOR.to_le_bytes() {
        return Err(invalid("Version PVXD ou de générateur inconnue"));
    }
    Ok(payload)
}

fn read_manifest(directory: &Path) -> io::Result<u64> {
    let bytes = read_bounded(File::open(directory.join(MANIFEST))?, MANIFEST_BYTES)?;
    let payload = checked_payload(&bytes, b"PVXD", 16)?;
    Ok(u64::from_le_bytes(payload[8..16].try_into().unwrap()))
}

fn prefix(magic: &[u8; 4], seed: u64, capacity: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend(magic);
    bytes.extend(VERSION.to_le_bytes());
    bytes.extend(GENERATOR.to_le_bytes());
    bytes.extend(seed.to_le_bytes());
    bytes
}

fn region_bytes(seed: u64, region: ChunkPos, records: &[u32]) -> Vec<u8> {
    let mut bytes = prefix(b"PVRG", seed, REGION_HEADER + 4 * records.len() + CHECKSUM);
    for value in region {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend((records.len() as u32).to_le_bytes());
    for record in records {
        bytes.extend(record.to_le_bytes());
    }
    bytes.extend(crc32fast::hash(&bytes).to_le_bytes());
    bytes
}

fn initialize(path: &Path, directory: &Path, default_seed: u64) -> io::Result<u64> {
    let legacy = match File::open(path) {
        Ok(file) => Some(decode(&read_bounded(file, MAX_FILE)?)?),
        Err(err) if err.kind() == ErrorKind::NotFound => None,
        Err(err) => return Err(err),
    };
    let seed = legacy.as_ref().map_or(default_seed, World::seed);
    let parent = directory_for(directory)?;
    let staged = Builder::new()
        .prefix(".pvxd-migration-")
        .tempdir_in(parent)?;
    if let Some(world) = legacy {
        // The source v1 is bounded to the original 128 × 64 × 128 valley.
        let mut regions: BTreeMap<ChunkPos, Vec<u32>> = BTreeMap::new();
        for (pos, block) in world.edits() {
            let location = Location::new(chunk_of(pos))?;
            regions
                .entry(location.region)
                .or_default()
                .push(location.pack(pos, block));
        }
        drop(world);
        for (region, mut edits) in regions {
            edits.sort_unstable();
            commit_file(
                &region_path(staged.path(), region),
                &region_bytes(seed, region, &edits),
            )?;
        }
    }
    let mut manifest = prefix(b"PVXD", seed, MANIFEST_BYTES);
    manifest.extend(crc32fast::hash(&manifest).to_le_bytes());
    commit_file(&staged.path().join(MANIFEST), &manifest)?;
    #[cfg(test)]
    interrupt_at("publish-world");
    fs::rename(staged.path(), directory)?;
    sync_directory(parent)?;
    Ok(seed)
}

fn commit_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = directory_for(path)?;
    let mut file = NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    #[cfg(test)]
    interrupt_at("replace-file");
    file.persist(path)?;
    sync_directory(parent)?;
    #[cfg(test)]
    interrupt_at("committed-file");
    Ok(())
}

fn sync_directory(directory: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(directory)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = directory;
    Ok(())
}

#[cfg(test)]
fn interrupt_at(point: &str) {
    if std::env::var("PRIMAL_REGIONAL_TEST_INTERRUPT").as_deref() == Ok(point) {
        // Skip destructors at an exact transaction boundary.
        std::process::exit(75);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save;
    use std::process::Command;

    #[test]
    fn process_exit_at_region_and_migration_commit_boundaries() {
        const CHILD_PATH: &str = "PRIMAL_REGIONAL_TEST_WORLD";
        const CHUNK_POS: ChunkPos = [0, 3, 0];
        const POS: Pos = [0, 63, 0];
        if let Some(path) = std::env::var_os(CHILD_PATH) {
            // The process exits from inside initialize() or commit_file().
            let store = RegionStore::open(Path::new(&path), 1).unwrap();
            store.save_chunk(CHUNK_POS, &[(POS, 5)]).unwrap();
            panic!("Le sous-processus aurait dû être interrompu");
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vallee.pvx");
        let mut world = World::new(7049);
        assert!(world.set(POS, 3));
        assert!(world.set([-1, 63, -1], 7));
        save(&path, &world).unwrap();
        let original = fs::read(&path).unwrap();
        let stop_child = |point| {
            let child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "region::tests::process_exit_at_region_and_migration_commit_boundaries",
                ])
                .env(CHILD_PATH, &path)
                .env("PRIMAL_REGIONAL_TEST_INTERRUPT", point)
                .output()
                .unwrap();
            assert_eq!(child.status.code(), Some(75), "{child:?}");
        };
        stop_child("publish-world");
        assert!(!RegionStore::directory(&path).exists());
        assert_eq!(fs::read(&path).unwrap(), original);
        let store = RegionStore::open(&path, 1).unwrap();
        assert_eq!(store.seed(), 7049);
        assert_eq!(store.load_chunk(CHUNK_POS).unwrap(), [(POS, 3)]);
        drop(store);
        for (point, expected) in [("replace-file", 3), ("committed-file", 5)] {
            stop_child(point);
            let store = RegionStore::open(&path, 1).unwrap();
            assert_eq!(store.load_chunk(CHUNK_POS).unwrap(), [(POS, expected)]);
            assert_eq!(store.load_chunk([-1, 3, -1]).unwrap(), [([-1, 63, -1], 7)]);
            assert_eq!(fs::read(&path).unwrap(), original);
        }
    }
}
