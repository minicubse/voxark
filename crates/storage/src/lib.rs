//! Terrain snapshots for the bounded valley. See docs/ARCHITECTURE.md for the wire format.
use primal_voxel::{GENERATOR_VERSION, MAX_BLOCK, Pos, World};
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::path::Path;
use tempfile::NamedTempFile;

const MAGIC: &[u8; 4] = b"PVXD";
const FORMAT: u16 = 1;
const GENERATOR: u16 = {
    assert!(GENERATOR_VERSION <= u16::MAX as u32);
    GENERATOR_VERSION as u16
};
const HEADER: usize = 20;
const RECORD: usize = 4;
const CHECKSUM: usize = 4;
const MAX_RECORDS: usize = 128 * 64 * 128;
const MAX_FILE: usize = HEADER + MAX_RECORDS * RECORD + CHECKSUM;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, message)
}

fn directory_for(path: &Path) -> io::Result<&Path> {
    if path.file_name().is_none() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "Chemin de sauvegarde invalide",
        ));
    }
    Ok(path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new(".")))
}

/// Kept open for the entire interactive session. The OS releases the lock on process exit.
pub struct WorldLock {
    _handle: File,
}

impl WorldLock {
    pub fn acquire(path: &Path) -> io::Result<Self> {
        fs::create_dir_all(directory_for(path)?)?;
        let mut lock_name = path.as_os_str().to_owned();
        lock_name.push(".lock");
        let handle = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_name)?;
        match handle.try_lock() {
            Ok(()) => Ok(Self { _handle: handle }),
            Err(fs::TryLockError::WouldBlock) => Err(io::Error::new(
                ErrorKind::WouldBlock,
                "Ce monde est déjà ouvert",
            )),
            Err(fs::TryLockError::Error(err)) => Err(err),
        }
    }
}

/// Only a missing snapshot starts a fresh world. A damaged or unsupported file is an error.
pub fn load_or_create(path: &Path, seed: u64) -> io::Result<World> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(World::new(seed)),
        Err(err) => return Err(err),
    };
    let mut bytes = Vec::new();
    file.take((MAX_FILE + 1) as u64).read_to_end(&mut bytes)?;
    decode(&bytes)
}

fn decode(bytes: &[u8]) -> io::Result<World> {
    if !(HEADER + CHECKSUM..=MAX_FILE).contains(&bytes.len()) || &bytes[..4] != MAGIC {
        return Err(invalid("Signature ou taille de sauvegarde invalide"));
    }
    let (payload, checksum) = bytes.split_at(bytes.len() - CHECKSUM);
    if crc32fast::hash(payload).to_le_bytes() != checksum {
        return Err(invalid("Somme de contrôle incorrecte"));
    }
    if payload[4..6] != FORMAT.to_le_bytes() || payload[6..8] != GENERATOR.to_le_bytes() {
        return Err(invalid("Version de sauvegarde ou de générateur inconnue"));
    }
    let seed = u64::from_le_bytes(payload[8..16].try_into().unwrap());
    let records = u32::from_le_bytes(payload[16..20].try_into().unwrap()) as usize;
    if records > MAX_RECORDS || payload.len() != HEADER + records * RECORD {
        return Err(invalid("Nombre de modifications incohérent"));
    }
    let mut world = World::new(seed);
    let mut last: Option<Pos> = None;
    for entry in payload[HEADER..].as_chunks::<RECORD>().0 {
        let position = [
            entry[0] as i8 as i32,
            entry[1] as i8 as i32,
            entry[2] as i8 as i32,
        ];
        if !World::contains(position) || entry[3] > MAX_BLOCK || last.is_some_and(|p| p >= position)
        {
            return Err(invalid(
                "Modification voxel invalide, dupliquée ou désordonnée",
            ));
        }
        if !world.set(position, entry[3]) {
            return Err(invalid("Modification identique au terrain généré"));
        }
        last = Some(position);
    }
    Ok(world)
}

/// Writes a complete staging file beside its destination, then atomically replaces the snapshot.
pub fn save(path: &Path, world: &World) -> io::Result<()> {
    save_snapshot(path, world.seed(), &world.edits())
}

/// A detached, ordered v1 snapshot for a writer outside the simulation thread.
pub fn save_snapshot(path: &Path, seed: u64, edits: &[(Pos, u8)]) -> io::Result<()> {
    if edits.len() > MAX_RECORDS
        || edits
            .iter()
            .any(|(p, b)| !World::contains(*p) || *b > MAX_BLOCK)
        || edits.windows(2).any(|w| w[0].0 >= w[1].0)
    {
        return Err(invalid("Snapshot hors format PVXD v1"));
    }
    let dir = directory_for(path)?;
    fs::create_dir_all(dir)?;
    stage_snapshot(dir, seed, edits)?.persist(path)?;
    #[cfg(unix)]
    File::open(dir)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
fn stage(dir: &Path, world: &World) -> io::Result<NamedTempFile> {
    stage_snapshot(dir, world.seed(), &world.edits())
}

fn stage_snapshot(dir: &Path, seed: u64, edits: &[(Pos, u8)]) -> io::Result<NamedTempFile> {
    let mut buffer = Vec::with_capacity(HEADER + RECORD * edits.len() + CHECKSUM);
    buffer.extend(MAGIC);
    buffer.extend(FORMAT.to_le_bytes());
    buffer.extend(GENERATOR.to_le_bytes());
    buffer.extend(seed.to_le_bytes());
    buffer.extend((edits.len() as u32).to_le_bytes());
    for &([x, y, z], block) in edits {
        buffer.extend([x as i8 as u8, y as i8 as u8, z as i8 as u8, block]);
    }
    buffer.extend(crc32fast::hash(&buffer).to_le_bytes());
    let mut staged = NamedTempFile::new_in(dir)?;
    staged.write_all(&buffer)?;
    staged.as_file().sync_all()?;
    Ok(staged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, exit};

    #[test]
    fn process_exit_before_replace_keeps_the_committed_edit() {
        const CHILD_DIR: &str = "PRIMAL_TEST_ABORT_BEFORE_RENAME";
        let ground = [1, 0, 1];
        if let Some(dir) = std::env::var_os(CHILD_DIR) {
            let dir = Path::new(&dir);
            let path = dir.join("world.pvx");
            let _lock = WorldLock::acquire(&path).unwrap();
            let mut world = load_or_create(&path, 0).unwrap();
            assert_eq!(world.seed(), 617);
            assert_eq!(world.get(ground), 0);
            assert!(world.set(ground, 5));
            let _unpublished = stage(dir, &world).unwrap();
            exit(75); // Skip destructors between sync_all and persist, like a stopped process.
        }
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("world.pvx");
        let mut world = World::new(617);
        assert!(world.set(ground, 0));
        save(&path, &world).unwrap();
        drop(world);
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::process_exit_before_replace_keeps_the_committed_edit",
            ])
            .env(CHILD_DIR, temp.path())
            .output()
            .unwrap();
        assert_eq!(
            child.status.code(),
            Some(75),
            "{}",
            String::from_utf8_lossy(&child.stdout)
        );
        let _lock = WorldLock::acquire(&path).unwrap();
        assert_eq!(load_or_create(&path, 0).unwrap().get(ground), 0);
    }
}
