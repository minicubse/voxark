//! Durable server transactions layered on the unchanged 1C regional terrain format.
//!
//! Own this handle on one disk worker. A durable redb transaction records revisions,
//! players, opaque inventory bytes, operation identity and complete pending chunk
//! snapshots together. Only then are the snapshots materialized in RegionStore.
//! Opening replays pending snapshots before exposing any state. Readers must use
//! this handle, and the caller must not write to the shared RegionStore directly.
//! A successful commit can be published to clients immediately. An I/O error after
//! the durable commit has an ambiguous outcome: retry the SAME operation id and
//! payload, or reopen, rather than submit the edit with a new id.

use primal_storage::RegionStore;
use primal_voxel::{CHUNK, ChunkPos, MAX_BLOCK, Pos, chunk_at};
use redb::{Database, Durability, ReadableDatabase, ReadableTable, TableDefinition};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

const META: TableDefinition<u8, u64> = TableDefinition::new("metadata");
const REVISIONS: TableDefinition<&[u8], u64> = TableDefinition::new("revisions");
const PLAYERS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("players");
const INVENTORIES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("inventories");
const OPERATIONS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("operations");
const PENDING: TableDefinition<u8, &[u8]> = TableDefinition::new("pending");
const FORMAT: u64 = 1;
const MAX_UPDATES: usize = 256;
const MAX_INVENTORY: usize = 64 * 1024;
const MAX_TRANSACTION: usize = 32 * 1024 * 1024;

pub type PlayerToken = [u8; 32];

/// Callers choose a fresh opaque id per logical operation and retain it on retry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationId(pub [u8; 32]);

impl OperationId {
    pub fn player_edit(token: &PlayerToken, sequence: u64) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"primal-edit-v1");
        digest.update(token);
        digest.update(sequence.to_le_bytes());
        Self(digest.finalize().into())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerState {
    pub id: u64,
    pub position: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub flying: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChunkState {
    pub revision: u64,
    pub edits: Vec<(Pos, u8)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChunkUpdate {
    pub chunk: ChunkPos,
    pub expected_revision: u64,
    /// Complete delta snapshot, including preexisting regional/v1 edits.
    pub edits: Vec<(Pos, u8)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerUpdate {
    pub token: PlayerToken,
    pub state: PlayerState,
}

/// Reserved application data; the ledger imposes no inventory/gameplay semantics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InventoryUpdate {
    pub token: PlayerToken,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Transaction {
    pub id: OperationId,
    pub chunks: Vec<ChunkUpdate>,
    pub players: Vec<PlayerUpdate>,
    pub inventories: Vec<InventoryUpdate>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitReceipt {
    pub duplicate: bool,
    pub revisions: Vec<(ChunkPos, u64)>,
}

pub struct Ledger {
    // Database closes before the last regional lock is released.
    database: Database,
    regions: RegionStore,
    ready: bool,
}

fn db_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn chunk_key(chunk: ChunkPos) -> [u8; 12] {
    let mut key = [0; 12];
    for (word, value) in key.as_chunks_mut::<4>().0.iter_mut().zip(chunk) {
        word.copy_from_slice(&value.to_le_bytes());
    }
    key
}

fn decode_chunk_key(bytes: &[u8]) -> io::Result<ChunkPos> {
    let mut reader = Reader(bytes);
    let chunk = reader.chunk()?;
    reader.end()?;
    Ok(chunk)
}

impl Ledger {
    /// Shares the already locked regional store, preserving the legacy import path.
    pub fn open(world_path: &Path, regions: RegionStore) -> io::Result<Self> {
        let database = Database::create(Self::path(world_path)).map_err(db_error)?;
        let mut write = database.begin_write().map_err(db_error)?;
        write
            .set_durability(Durability::Immediate)
            .map_err(db_error)?;
        {
            let mut meta = write.open_table(META).map_err(db_error)?;
            let version = meta.get(0).map_err(db_error)?.map(|v| v.value());
            let seed = meta.get(1).map_err(db_error)?.map(|v| v.value());
            if version.is_some_and(|v| v != FORMAT)
                || seed.is_some_and(|v| v != regions.seed())
                || version.is_some() != seed.is_some()
            {
                return Err(invalid("Version ou seed de journal incompatible"));
            }
            meta.insert(0, FORMAT).map_err(db_error)?;
            meta.insert(1, regions.seed()).map_err(db_error)?;
            write.open_table(REVISIONS).map_err(db_error)?;
            write.open_table(PLAYERS).map_err(db_error)?;
            write.open_table(INVENTORIES).map_err(db_error)?;
            write.open_table(OPERATIONS).map_err(db_error)?;
            write.open_table(PENDING).map_err(db_error)?;
        }
        write.commit().map_err(db_error)?;
        let mut ledger = Self {
            database,
            regions,
            ready: false,
        };
        ledger.recover()?;
        Ok(ledger)
    }

    pub fn path(world_path: &Path) -> PathBuf {
        RegionStore::directory(world_path).join("server.redb")
    }

    pub fn seed(&self) -> u64 {
        self.regions.seed()
    }

    fn ensure_ready(&self) -> io::Result<()> {
        if self.ready {
            Ok(())
        } else {
            Err(io::Error::other(
                "Journal à rejouer avant toute lecture; retry commit ou rouvrir",
            ))
        }
    }

    pub fn load_chunk(&self, chunk: ChunkPos) -> io::Result<ChunkState> {
        self.ensure_ready()?;
        let read = self.database.begin_read().map_err(db_error)?;
        let table = read.open_table(REVISIONS).map_err(db_error)?;
        let revision = table
            .get(chunk_key(chunk).as_slice())
            .map_err(db_error)?
            .map_or(0, |v| v.value());
        Ok(ChunkState {
            revision,
            edits: self.regions.load_chunk(chunk)?,
        })
    }

    pub fn revisions(&self) -> io::Result<Vec<(ChunkPos, u64)>> {
        self.ensure_ready()?;
        let read = self.database.begin_read().map_err(db_error)?;
        let table = read.open_table(REVISIONS).map_err(db_error)?;
        table
            .iter()
            .map_err(db_error)?
            .map(|entry| {
                let (key, value) = entry.map_err(db_error)?;
                Ok((decode_chunk_key(key.value())?, value.value()))
            })
            .collect()
    }

    pub fn player(&self, token: &PlayerToken) -> io::Result<Option<PlayerState>> {
        self.ensure_ready()?;
        let read = self.database.begin_read().map_err(db_error)?;
        let table = read.open_table(PLAYERS).map_err(db_error)?;
        table
            .get(token.as_slice())
            .map_err(db_error)?
            .map(|v| decode_player(v.value()))
            .transpose()
    }

    pub fn players(&self) -> io::Result<Vec<(PlayerToken, PlayerState)>> {
        self.ensure_ready()?;
        let read = self.database.begin_read().map_err(db_error)?;
        let table = read.open_table(PLAYERS).map_err(db_error)?;
        table
            .iter()
            .map_err(db_error)?
            .map(|entry| {
                let (key, value) = entry.map_err(db_error)?;
                let token = key
                    .value()
                    .try_into()
                    .map_err(|_| invalid("Token stocké invalide"))?;
                Ok((token, decode_player(value.value())?))
            })
            .collect()
    }

    pub fn inventory(&self, token: &PlayerToken) -> io::Result<Option<Vec<u8>>> {
        self.ensure_ready()?;
        let read = self.database.begin_read().map_err(db_error)?;
        let table = read.open_table(INVENTORIES).map_err(db_error)?;
        Ok(table
            .get(token.as_slice())
            .map_err(db_error)?
            .map(|v| v.value().to_vec()))
    }

    /// A new token receives a stable, monotonically allocated id; initial.id is ignored.
    /// An existing token always returns its saved state, ignoring the initial state.
    pub fn get_or_create(
        &mut self,
        token: PlayerToken,
        mut initial: PlayerState,
    ) -> io::Result<PlayerState> {
        self.recover()?;
        if let Some(saved) = self.player(&token)? {
            return Ok(saved);
        }
        let mut write = self.database.begin_write().map_err(db_error)?;
        write
            .set_durability(Durability::Immediate)
            .map_err(db_error)?;
        {
            let mut meta = write.open_table(META).map_err(db_error)?;
            let previous = meta.get(2).map_err(db_error)?.map_or(0, |v| v.value());
            initial.id = previous
                .checked_add(1)
                .ok_or_else(|| input("Identifiants joueurs épuisés"))?;
            validate_player(&initial)?;
            meta.insert(2, initial.id).map_err(db_error)?;
            let bytes = encode_player(&initial);
            write
                .open_table(PLAYERS)
                .map_err(db_error)?
                .insert(token.as_slice(), bytes.as_slice())
                .map_err(db_error)?;
        }
        write.commit().map_err(db_error)?;
        Ok(initial)
    }

    /// Bounds: 256 updates per category, 4096 deltas per chunk, 64 KiB per inventory,
    /// 32 MiB encoded transaction. Idempotency retains the exact request bytes on disk;
    /// reusing an id with a different request (including ordering) is rejected.
    pub fn commit(&mut self, transaction: &Transaction) -> io::Result<CommitReceipt> {
        self.recover()?;
        validate(transaction)?;
        let bytes = encode(transaction);
        if bytes.len() > MAX_TRANSACTION {
            return Err(input("Transaction trop grande"));
        }
        let mut write = self.database.begin_write().map_err(db_error)?;
        write
            .set_durability(Durability::Immediate)
            .map_err(db_error)?;
        {
            let operations = write.open_table(OPERATIONS).map_err(db_error)?;
            if let Some(previous) = operations
                .get(transaction.id.0.as_slice())
                .map_err(db_error)?
            {
                if previous.value() != bytes {
                    return Err(input(
                        "Identifiant opération réutilisé avec un autre contenu",
                    ));
                }
                return Ok(receipt(transaction, true));
            }
        }
        {
            let mut revisions = write.open_table(REVISIONS).map_err(db_error)?;
            for update in &transaction.chunks {
                let key = chunk_key(update.chunk);
                let current = revisions
                    .get(key.as_slice())
                    .map_err(db_error)?
                    .map_or(0, |v| v.value());
                if current != update.expected_revision {
                    return Err(input("Conflit de révision terrain"));
                }
                revisions
                    .insert(key.as_slice(), current + 1)
                    .map_err(db_error)?;
            }
            write_players(&write, &transaction.players)?;
            let players = write.open_table(PLAYERS).map_err(db_error)?;
            let mut inventories = write.open_table(INVENTORIES).map_err(db_error)?;
            for update in &transaction.inventories {
                if players
                    .get(update.token.as_slice())
                    .map_err(db_error)?
                    .is_none()
                {
                    return Err(input("Inventaire sans joueur"));
                }
                inventories
                    .insert(update.token.as_slice(), update.data.as_slice())
                    .map_err(db_error)?;
            }
            write
                .open_table(OPERATIONS)
                .map_err(db_error)?
                .insert(transaction.id.0.as_slice(), bytes.as_slice())
                .map_err(db_error)?;
            write
                .open_table(PENDING)
                .map_err(db_error)?
                .insert(0, bytes.as_slice())
                .map_err(db_error)?;
        }
        interrupt_at("before-wal");
        // Block all reads even if commit itself returns an ambiguous I/O failure.
        self.ready = false;
        write.commit().map_err(db_error)?;
        interrupt_at("after-wal");
        self.recover()?;
        Ok(receipt(transaction, false))
    }

    /// Atomic periodic player snapshots, without permanent idempotency receipts.
    pub fn save_players(&mut self, updates: &[PlayerUpdate]) -> io::Result<()> {
        self.recover()?;
        let transaction = Transaction {
            id: OperationId([0; 32]),
            chunks: vec![],
            players: updates.to_vec(),
            inventories: vec![],
        };
        validate(&transaction)?;
        let mut write = self.database.begin_write().map_err(db_error)?;
        write
            .set_durability(Durability::Immediate)
            .map_err(db_error)?;
        write_players(&write, updates)?;
        write.commit().map_err(db_error)
    }

    /// Finish a previously committed operation after an interrupted regional write.
    pub fn recover(&mut self) -> io::Result<()> {
        let pending = {
            let read = self.database.begin_read().map_err(db_error)?;
            let table = read.open_table(PENDING).map_err(db_error)?;
            table.get(0).map_err(db_error)?.map(|v| v.value().to_vec())
        };
        if let Some(bytes) = pending {
            self.ready = false;
            let transaction = decode(&bytes)?;
            for update in &transaction.chunks {
                self.regions.save_chunk(update.chunk, &update.edits)?;
                interrupt_at("after-region");
            }
            interrupt_at("before-clear");
            let mut write = self.database.begin_write().map_err(db_error)?;
            write
                .set_durability(Durability::Immediate)
                .map_err(db_error)?;
            write
                .open_table(PENDING)
                .map_err(db_error)?
                .remove(0)
                .map_err(db_error)?;
            write.commit().map_err(db_error)?;
        }
        self.ready = true;
        Ok(())
    }
}

fn receipt(transaction: &Transaction, duplicate: bool) -> CommitReceipt {
    CommitReceipt {
        duplicate,
        revisions: transaction
            .chunks
            .iter()
            .map(|u| (u.chunk, u.expected_revision + 1))
            .collect(),
    }
}

fn write_players(write: &redb::WriteTransaction, updates: &[PlayerUpdate]) -> io::Result<()> {
    let mut players = write.open_table(PLAYERS).map_err(db_error)?;
    let mut max_id = write
        .open_table(META)
        .map_err(db_error)?
        .get(2)
        .map_err(db_error)?
        .map_or(0, |v| v.value());
    for update in updates {
        if let Some(saved) = players.get(update.token.as_slice()).map_err(db_error)?
            && decode_player(saved.value())?.id != update.state.id
        {
            return Err(input("Identifiant joueur immuable"));
        }
        for entry in players.iter().map_err(db_error)? {
            let (token, value) = entry.map_err(db_error)?;
            if token.value() != update.token && decode_player(value.value())?.id == update.state.id
            {
                return Err(input("Identifiant joueur déjà attribué"));
            }
        }
        max_id = max_id.max(update.state.id);
        let value = encode_player(&update.state);
        players
            .insert(update.token.as_slice(), value.as_slice())
            .map_err(db_error)?;
    }
    write
        .open_table(META)
        .map_err(db_error)?
        .insert(2, max_id)
        .map_err(db_error)?;
    Ok(())
}

fn validate_player(state: &PlayerState) -> io::Result<()> {
    if state.id == 0
        || !state
            .position
            .into_iter()
            .chain([state.yaw, state.pitch])
            .all(f32::is_finite)
    {
        return Err(input("État joueur invalide"));
    }
    Ok(())
}

fn validate(transaction: &Transaction) -> io::Result<()> {
    if [
        transaction.chunks.len(),
        transaction.players.len(),
        transaction.inventories.len(),
    ]
    .into_iter()
    .any(|n| n > MAX_UPDATES)
    {
        return Err(input("Trop de mises à jour"));
    }
    let mut chunks = BTreeSet::new();
    for update in &transaction.chunks {
        if !chunks.insert(update.chunk)
            || update.expected_revision == u64::MAX
            || update
                .chunk
                .into_iter()
                .any(|v| v.checked_mul(CHUNK).is_none())
            || update.edits.len() > (CHUNK * CHUNK * CHUNK) as usize
        {
            return Err(input("Chunk ou révision invalide ou dupliqué"));
        }
        let mut positions = BTreeSet::new();
        for &(pos, block) in &update.edits {
            if block > MAX_BLOCK || chunk_at(pos) != update.chunk || !positions.insert(pos) {
                return Err(input("Delta voxel invalide ou dupliqué"));
            }
        }
    }
    let mut tokens = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for update in &transaction.players {
        validate_player(&update.state)?;
        if !tokens.insert(update.token) || !ids.insert(update.state.id) {
            return Err(input("Joueur dupliqué"));
        }
    }
    tokens.clear();
    for update in &transaction.inventories {
        if !tokens.insert(update.token) || update.data.len() > MAX_INVENTORY {
            return Err(input("Inventaire invalide ou dupliqué"));
        }
    }
    Ok(())
}

fn encode_player(state: &PlayerState) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(29);
    bytes.extend(state.id.to_le_bytes());
    for value in state.position.into_iter().chain([state.yaw, state.pitch]) {
        bytes.extend(value.to_le_bytes());
    }
    bytes.push(u8::from(state.flying));
    bytes
}

fn decode_player(bytes: &[u8]) -> io::Result<PlayerState> {
    let mut reader = Reader(bytes);
    let state = reader.player()?;
    reader.end()?;
    validate_player(&state).map_err(|_| invalid("État joueur stocké invalide"))?;
    Ok(state)
}

fn encode(transaction: &Transaction) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(b"PVLT\x01\0");
    bytes.extend(transaction.id.0);
    bytes.extend((transaction.chunks.len() as u32).to_le_bytes());
    for update in &transaction.chunks {
        bytes.extend(chunk_key(update.chunk));
        bytes.extend(update.expected_revision.to_le_bytes());
        bytes.extend((update.edits.len() as u32).to_le_bytes());
        for &(pos, block) in &update.edits {
            bytes.extend(chunk_key(pos));
            bytes.push(block);
        }
    }
    bytes.extend((transaction.players.len() as u32).to_le_bytes());
    for update in &transaction.players {
        bytes.extend(update.token);
        bytes.extend(encode_player(&update.state));
    }
    bytes.extend((transaction.inventories.len() as u32).to_le_bytes());
    for update in &transaction.inventories {
        bytes.extend(update.token);
        bytes.extend((update.data.len() as u32).to_le_bytes());
        bytes.extend(&update.data);
    }
    bytes
}

fn decode(bytes: &[u8]) -> io::Result<Transaction> {
    if bytes.len() > MAX_TRANSACTION {
        return Err(invalid("Journal trop grand"));
    }
    let mut reader = Reader(bytes);
    if reader.take(6)? != b"PVLT\x01\0" {
        return Err(invalid("Version de journal inconnue"));
    }
    let id = OperationId(reader.array()?);
    let mut chunks = Vec::new();
    for _ in 0..reader.count(MAX_UPDATES)? {
        let chunk = reader.chunk()?;
        let expected_revision = reader.u64()?;
        let mut edits = Vec::new();
        for _ in 0..reader.count((CHUNK * CHUNK * CHUNK) as usize)? {
            edits.push((reader.chunk()?, reader.array::<1>()?[0]));
        }
        chunks.push(ChunkUpdate {
            chunk,
            expected_revision,
            edits,
        });
    }
    let mut players = Vec::new();
    for _ in 0..reader.count(MAX_UPDATES)? {
        players.push(PlayerUpdate {
            token: reader.array()?,
            state: reader.player()?,
        });
    }
    let mut inventories = Vec::new();
    for _ in 0..reader.count(MAX_UPDATES)? {
        let token = reader.array()?;
        let count = reader.count(MAX_INVENTORY)?;
        inventories.push(InventoryUpdate {
            token,
            data: reader.take(count)?.to_vec(),
        });
    }
    reader.end()?;
    let transaction = Transaction {
        id,
        chunks,
        players,
        inventories,
    };
    validate(&transaction).map_err(|_| invalid("Journal transactionnel invalide"))?;
    Ok(transaction)
}

struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> io::Result<&'a [u8]> {
        if count > self.0.len() {
            return Err(invalid("Journal tronqué"));
        }
        let (value, tail) = self.0.split_at(count);
        self.0 = tail;
        Ok(value)
    }
    fn array<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }
    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }
    fn count(&mut self, max: usize) -> io::Result<usize> {
        let count = u32::from_le_bytes(self.array()?) as usize;
        if count > max {
            return Err(invalid("Nombre d'enregistrements invalide"));
        }
        Ok(count)
    }
    fn chunk(&mut self) -> io::Result<ChunkPos> {
        Ok([
            i32::from_le_bytes(self.array()?),
            i32::from_le_bytes(self.array()?),
            i32::from_le_bytes(self.array()?),
        ])
    }
    fn player(&mut self) -> io::Result<PlayerState> {
        let id = self.u64()?;
        let mut values = [0.0; 5];
        for value in &mut values {
            *value = f32::from_le_bytes(self.array()?);
        }
        let flying = match self.array::<1>()?[0] {
            0 => false,
            1 => true,
            _ => return Err(invalid("Drapeau de vol invalide")),
        };
        Ok(PlayerState {
            id,
            position: [values[0], values[1], values[2]],
            yaw: values[3],
            pitch: values[4],
            flying,
        })
    }
    fn end(&self) -> io::Result<()> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(invalid("Octets superflus dans le journal"))
        }
    }
}

#[cfg(not(test))]
fn interrupt_at(_: &str) {}

#[cfg(test)]
fn interrupt_at(point: &str) {
    if std::env::var("PRIMAL_LEDGER_TEST_INTERRUPT").as_deref() == Ok(point) {
        std::process::exit(76);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn sample() -> Transaction {
        Transaction {
            id: OperationId([7; 32]),
            chunks: vec![
                ChunkUpdate {
                    chunk: [0, 3, 0],
                    expected_revision: 0,
                    edits: vec![([0, 63, 0], 5)],
                },
                ChunkUpdate {
                    chunk: [-5, 3, 0],
                    expected_revision: 0,
                    edits: vec![([-65, 63, 0], 7)],
                },
            ],
            players: vec![PlayerUpdate {
                token: [42; 32],
                state: PlayerState {
                    id: 1,
                    position: [1.5, 63.0, -9.0],
                    yaw: 1.25,
                    pitch: -0.5,
                    flying: true,
                },
            }],
            inventories: vec![InventoryUpdate {
                token: [42; 32],
                data: vec![2, 3, 4],
            }],
        }
    }

    #[test]
    fn crashes_before_and_after_wal_and_between_regions() {
        if let Some(path) = std::env::var_os("PRIMAL_LEDGER_TEST_WORLD") {
            let path = Path::new(&path);
            let mut ledger = Ledger::open(path, RegionStore::open(path, 1).unwrap()).unwrap();
            ledger.commit(&sample()).unwrap();
            panic!("Child should have exited at the selected boundary");
        }
        for point in ["before-wal", "after-wal", "after-region", "before-clear"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("world.pvx");
            {
                let regions = RegionStore::open(&path, 7049).unwrap();
                regions.save_chunk([1, 3, 0], &[([16, 63, 0], 3)]).unwrap();
                Ledger::open(&path, regions).unwrap();
            }
            let crash = |boundary: &str| {
                let child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "tests::crashes_before_and_after_wal_and_between_regions",
                    ])
                    .env("PRIMAL_LEDGER_TEST_WORLD", &path)
                    .env("PRIMAL_LEDGER_TEST_INTERRUPT", boundary)
                    .output()
                    .unwrap();
                assert_eq!(child.status.code(), Some(76), "{point}: {child:?}");
            };
            crash(point);
            if point == "after-wal" {
                // Crash during replay itself, then replay the same snapshot again.
                crash("after-region");
            }
            let mut ledger = Ledger::open(&path, RegionStore::open(&path, 1).unwrap()).unwrap();
            let committed = point != "before-wal";
            assert_eq!(
                ledger.player(&[42; 32]).unwrap(),
                committed.then(|| sample().players[0].state.clone()),
                "{point}"
            );
            assert_eq!(
                ledger.inventory(&[42; 32]).unwrap(),
                committed.then(|| vec![2, 3, 4]),
                "{point}"
            );
            for update in sample().chunks {
                let chunk = ledger.load_chunk(update.chunk).unwrap();
                assert_eq!(chunk.revision, u64::from(committed), "{point}");
                assert_eq!(
                    chunk.edits,
                    if committed { update.edits } else { vec![] },
                    "{point}"
                );
            }
            assert_eq!(
                ledger.load_chunk([1, 3, 0]).unwrap().edits,
                vec![([16, 63, 0], 3)]
            );
            assert_eq!(ledger.commit(&sample()).unwrap().duplicate, committed);
        }
    }

    #[test]
    fn committed_io_failure_blocks_reads_until_replay_then_retry_is_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("world.pvx");
        let mut ledger = Ledger::open(&path, RegionStore::open(&path, 1).unwrap()).unwrap();
        let obstruction = RegionStore::directory(&path).join("r.0.0.0.pvr");
        std::fs::create_dir(&obstruction).unwrap();
        assert!(ledger.commit(&sample()).is_err());
        assert!(ledger.player(&[42; 32]).is_err());
        assert!(ledger.load_chunk([0, 3, 0]).is_err());
        std::fs::remove_dir(&obstruction).unwrap();
        assert!(ledger.commit(&sample()).unwrap().duplicate);
        assert_eq!(
            ledger.player(&[42; 32]).unwrap(),
            Some(sample().players[0].state.clone())
        );
        assert_eq!(ledger.load_chunk([-5, 3, 0]).unwrap().revision, 1);
    }

    #[test]
    fn rejects_truncated_corrupt_and_excessive_pending_records() {
        let bytes = encode(&sample());
        for length in 0..bytes.len() {
            assert!(decode(&bytes[..length]).is_err());
        }
        assert_eq!(decode(&bytes).unwrap(), sample());
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(decode(&trailing).is_err());
        let mut excessive = bytes;
        excessive[38..42].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&excessive).is_err());
    }

    #[test]
    fn periodic_snapshots_are_atomic_and_do_not_accumulate_operation_receipts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("world.pvx");
        let mut ledger = Ledger::open(&path, RegionStore::open(&path, 1).unwrap()).unwrap();
        let mut players = sample().players;
        for yaw in [0.0, 1.0, 2.0, 3.0] {
            players[0].state.yaw = yaw;
            ledger.save_players(&players).unwrap();
        }
        let mut conflicting = players.clone();
        conflicting[0].state.position = [100.0; 3];
        let mut duplicate_id = players[0].clone();
        duplicate_id.token = [43; 32];
        conflicting.push(duplicate_id);
        assert!(ledger.save_players(&conflicting).is_err());
        assert_eq!(
            ledger.player(&[42; 32]).unwrap(),
            Some(players[0].state.clone())
        );
        let read = ledger.database.begin_read().unwrap();
        assert_eq!(
            read.open_table(OPERATIONS).unwrap().iter().unwrap().count(),
            0
        );
    }
}
