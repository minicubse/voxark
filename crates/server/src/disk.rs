//! One bounded blocking disk worker. The simulation polls only durable edit acks.
use primal_ledger::{ChunkUpdate, Ledger, OperationId, PlayerState, PlayerUpdate, Transaction};
use primal_storage::RegionStore;
use primal_voxel::{CHUNK, ChunkPos, Pos};
use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::thread::{self, JoinHandle};

const CAPACITY: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableEdit {
    pub token: [u8; 32],
    pub seq: u64,
    pub chunk: ChunkPos,
    pub revision: u64,
    pub duplicate: bool,
    pub rejected: bool,
}

enum Request {
    Edit {
        token: [u8; 32],
        seq: u64,
        update: ChunkUpdate,
    },
    Players(Vec<PlayerUpdate>),
    Register {
        token: [u8; 32],
        initial: PlayerState,
        reply: SyncSender<io::Result<PlayerState>>,
    },
}

pub struct Disk {
    requests: Option<SyncSender<Request>>,
    results: Receiver<io::Result<DurableEdit>>,
    worker: Option<JoinHandle<io::Result<()>>>,
    disconnected: bool,
}

type InitialState = (Disk, HashMap<ChunkPos, u64>, HashMap<[u8; 32], PlayerState>);

impl Disk {
    pub fn open(path: &Path, store: RegionStore) -> io::Result<InitialState> {
        let ledger = Ledger::open(path, store)?;
        let revisions = ledger.revisions()?.into_iter().collect();
        let players = ledger.players()?.into_iter().collect();
        let (send, receive) = mpsc::sync_channel(CAPACITY);
        let (results_send, results) = mpsc::sync_channel(CAPACITY);
        let worker = thread::Builder::new()
            .name("server-disk".into())
            .spawn(move || run(ledger, receive, results_send))?;
        Ok((
            Self {
                requests: Some(send),
                results,
                worker: Some(worker),
                disconnected: false,
            },
            revisions,
            players,
        ))
    }

    /// Queue acceptance is not a durability acknowledgement. WouldBlock changes
    /// nothing: retain the request and retry after polling. A logical retry uses
    /// identical token, seq, chunk, expected_revision and complete delta snapshot.
    pub fn submit_edit(
        &self,
        token: [u8; 32],
        seq: u64,
        chunk: ChunkPos,
        expected_revision: u64,
        edits: Vec<(Pos, u8)>,
    ) -> io::Result<()> {
        if edits.len() > (CHUNK * CHUNK * CHUNK) as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Snapshot de chunk trop grand",
            ));
        }
        self.submit(Request::Edit {
            token,
            seq,
            update: ChunkUpdate {
                chunk,
                expected_revision,
                edits,
            },
        })
    }

    /// Periodic atomic upsert, with no permanent operation receipts. Errors are
    /// emitted by poll() and retained for finish(), even if poll consumed them.
    pub fn save_players(&self, players: Vec<PlayerUpdate>) -> io::Result<()> {
        if players.len() > 256 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Trop de joueurs dans le snapshot",
            ));
        }
        self.submit(Request::Players(players))
    }

    /// Welcome may be sent only after this receiver returns a durable state.
    /// The ledger allocates initial.id and reuses saved identity on reconnect.
    pub fn register(
        &self,
        token: [u8; 32],
        initial: PlayerState,
    ) -> io::Result<Receiver<io::Result<PlayerState>>> {
        let (reply, receive) = mpsc::sync_channel(1);
        self.submit(Request::Register {
            token,
            initial,
            reply,
        })?;
        Ok(receive)
    }

    fn submit(&self, request: Request) -> io::Result<()> {
        let send = self
            .requests
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "Worker disque fermé"))?;
        send.try_send(request).map_err(|error| match error {
            TrySendError::Full(_) => {
                io::Error::new(io::ErrorKind::WouldBlock, "File disque pleine")
            }
            TrySendError::Disconnected(_) => {
                io::Error::new(io::ErrorKind::BrokenPipe, "Worker disque arrêté")
            }
        })
    }

    pub fn poll(&mut self) -> Option<io::Result<DurableEdit>> {
        match self.results.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) if !self.disconnected => {
                self.disconnected = true;
                Some(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "Worker disque arrêté",
                )))
            }
            Err(TryRecvError::Disconnected) => None,
        }
    }

    /// Stop accepting requests, drain accepted work and acks, then join. Draining
    /// results while joining prevents a full bounded ack queue from deadlocking.
    pub fn finish(mut self) -> io::Result<()> {
        self.close()
    }

    fn close(&mut self) -> io::Result<()> {
        self.requests.take();
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        for _ in &self.results {}
        worker
            .join()
            .map_err(|_| io::Error::other("Panique du worker disque"))?
    }
}

impl Drop for Disk {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn run(
    mut ledger: Ledger,
    receive: Receiver<Request>,
    results: SyncSender<io::Result<DurableEdit>>,
) -> io::Result<()> {
    let mut first_error = None;
    for request in receive {
        let result = match request {
            Request::Edit { token, seq, update } => {
                let chunk = update.chunk;
                let expected_revision = update.expected_revision;
                match ledger.commit(&Transaction {
                    id: OperationId::player_edit(&token, seq),
                    chunks: vec![update],
                    players: vec![],
                    inventories: vec![],
                }) {
                    Ok(receipt) => Ok(Some(DurableEdit {
                        token,
                        seq,
                        chunk,
                        revision: receipt.revisions[0].1,
                        duplicate: receipt.duplicate,
                        rejected: false,
                    })),
                    Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
                        match ledger.load_chunk(chunk) {
                            Ok(snapshot) => Ok(Some(DurableEdit {
                                token,
                                seq,
                                chunk,
                                revision: snapshot.revision,
                                duplicate: false,
                                rejected: true,
                            })),
                            Err(read_error) if read_error.kind() == io::ErrorKind::InvalidInput => {
                                Ok(Some(DurableEdit {
                                    token,
                                    seq,
                                    chunk,
                                    revision: expected_revision,
                                    duplicate: false,
                                    rejected: true,
                                }))
                            }
                            Err(read_error) => Err(read_error),
                        }
                    }
                    Err(error) => Err(error),
                }
            }
            Request::Players(players) => ledger.save_players(&players).map(|()| None),
            Request::Register {
                token,
                initial,
                reply,
            } => match ledger.get_or_create(token, initial) {
                Ok(state) => {
                    let _ = reply.send(Ok(state));
                    Ok(None)
                }
                Err(error) => {
                    let _ = reply.send(Err(io::Error::new(error.kind(), error.to_string())));
                    Err(error)
                }
            },
        };
        let result = match result {
            Ok(Some(ack)) => Ok(ack),
            Ok(None) => continue,
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(io::Error::new(error.kind(), error.to_string()));
                }
                Err(error)
            }
        };
        if results.send(result).is_err() {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Récepteur disque fermé",
            ));
        }
    }
    first_error.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn player() -> PlayerUpdate {
        PlayerUpdate {
            token: [42; 32],
            state: PlayerState {
                id: 1,
                position: [0.5, 63.0, 0.5],
                yaw: 1.0,
                pitch: 0.5,
                flying: true,
            },
        }
    }

    fn ack(disk: &mut Disk) -> io::Result<DurableEdit> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(result) = disk.poll() {
                return result;
            }
            assert!(Instant::now() < deadline, "Disk acknowledgement timed out");
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn queued_shutdown_drains_and_restart_keeps_player_and_idempotent_edit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("world.pvx");
        let (disk, _, _) = Disk::open(&path, RegionStore::open(&path, 1).unwrap()).unwrap();
        disk.save_players(vec![player()]).unwrap();
        disk.submit_edit([42; 32], 3, [0, 3, 0], 0, vec![([0, 63, 0], 5)])
            .unwrap();
        disk.finish().unwrap();
        let (mut disk, revisions, players) =
            Disk::open(&path, RegionStore::open(&path, 1).unwrap()).unwrap();
        assert_eq!(revisions.get(&[0, 3, 0]), Some(&1));
        assert_eq!(players.get(&[42; 32]), Some(&player().state));
        disk.submit_edit([42; 32], 3, [0, 3, 0], 0, vec![([0, 63, 0], 5)])
            .unwrap();
        assert!(ack(&mut disk).unwrap().duplicate);
        disk.submit_edit([42; 32], 3, [0, 3, 0], 0, vec![([0, 63, 0], 7)])
            .unwrap();
        let rejection = ack(&mut disk).unwrap();
        assert!(rejection.rejected);
        assert_eq!(rejection.revision, 1);
        disk.finish().unwrap();
    }

    #[test]
    fn queues_are_bounded_and_finish_drains_unpolled_acknowledgements() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("world.pvx");
        let (disk, _, _) = Disk::open(&path, RegionStore::open(&path, 1).unwrap()).unwrap();
        let accepted = 2 * CAPACITY + 1;
        let deadline = Instant::now() + Duration::from_secs(10);
        for index in 0..accepted {
            loop {
                match disk.submit_edit(
                    [42; 32],
                    index as u64,
                    [index as i32, 3, 0],
                    0,
                    vec![([index as i32 * 16, 63, 0], 5)],
                ) {
                    Ok(()) => break,
                    Err(error) => {
                        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
                        assert!(Instant::now() < deadline);
                        thread::sleep(Duration::from_millis(1));
                    }
                }
            }
        }
        assert_eq!(
            disk.submit_edit([42; 32], 1000, [1000, 3, 0], 0, vec![])
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        disk.finish().unwrap();
        let (disk, revisions, _) = Disk::open(&path, RegionStore::open(&path, 1).unwrap()).unwrap();
        assert_eq!(revisions.len(), accepted);
        disk.finish().unwrap();
    }

    #[test]
    fn player_faults_are_polled_and_retained_by_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("world.pvx");
        let (mut disk, _, _) = Disk::open(&path, RegionStore::open(&path, 1).unwrap()).unwrap();
        let mut bad = player();
        bad.state.yaw = f32::NAN;
        disk.save_players(vec![bad]).unwrap();
        assert!(ack(&mut disk).is_err());
        assert!(disk.finish().is_err());
    }

    #[test]
    fn admission_ack_precedes_welcome_and_identity_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("world.pvx");
        let (disk, _, _) = Disk::open(&path, RegionStore::open(&path, 1).unwrap()).unwrap();
        let registered = disk
            .register([42; 32], player().state)
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert_eq!(registered.id, 1);
        disk.finish().unwrap();
        let (disk, _, _) = Disk::open(&path, RegionStore::open(&path, 1).unwrap()).unwrap();
        let mut initial = player().state;
        initial.id = 999;
        initial.position = [99.0; 3];
        assert_eq!(
            disk.register([42; 32], initial)
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap(),
            registered
        );
        disk.finish().unwrap();
    }
}
