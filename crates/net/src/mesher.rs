//! Bounded meshing of authoritative snapshots for both graphical clients.
use crate::Replica;
use primal_voxel::{ChunkPos, MeshSnapshot, chunk_at};
use std::{collections::BTreeSet, sync::mpsc};

pub struct ClientMesher<P> {
    jobs: mpsc::SyncSender<MeshSnapshot>,
    results: mpsc::Receiver<(ChunkPos, u64, P)>,
    inflight: BTreeSet<ChunkPos>,
    dirty: BTreeSet<ChunkPos>,
}
impl<P: Send + 'static> ClientMesher<P> {
    pub fn new(prepare: impl Fn(&MeshSnapshot) -> P + Send + 'static) -> Self {
        let (jobs, receive) = mpsc::sync_channel::<MeshSnapshot>(4);
        let (send, results) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(s) = receive.recv() {
                if send.send((s.chunk(), s.revision(), prepare(&s))).is_err() {
                    break;
                }
            }
        });
        Self {
            jobs,
            results,
            inflight: BTreeSet::new(),
            dirty: BTreeSet::new(),
        }
    }
    pub fn dirty(&mut self, chunks: impl IntoIterator<Item = ChunkPos>) {
        self.dirty.extend(chunks);
    }
    pub fn pending(&self) -> usize {
        self.dirty.union(&self.inflight).count()
    }
    pub fn tick(&mut self, replica: &Replica, position: [f32; 3]) -> Vec<(ChunkPos, Option<P>)> {
        let mut updates = Vec::new();
        while let Ok((c, rev, payload)) = self.results.try_recv() {
            self.inflight.remove(&c);
            if replica.world.mesh_revision(c) == Some(rev) {
                updates.push((c, Some(payload)));
            } else if replica.world.is_resident(c) {
                self.dirty.insert(c);
            }
        }
        let center = chunk_at(position.map(|v| v.floor() as i32));
        let mut pending: Vec<_> = self.dirty.iter().copied().collect();
        pending.sort_by_key(|c| c[0].abs_diff(center[0]) + c[2].abs_diff(center[2]));
        for c in pending {
            if !replica.world.is_resident(c) {
                self.dirty.remove(&c);
                updates.push((c, None));
                continue;
            }
            if self.inflight.len() >= 4 || self.inflight.contains(&c) {
                continue;
            }
            let Some(s) = replica.world.mesh_snapshot(c) else {
                continue;
            };
            if self.jobs.try_send(s).is_err() {
                break;
            }
            self.inflight.insert(c);
            self.dirty.remove(&c);
        }
        updates
    }
}
