//! One project held in memory: nodes with live indexes, the snapshot resources
//! (tokens, styles, assets, fonts, notes) and the revision.
mod dump;
mod load;
mod publication;
mod reads;
mod slice;
mod staged;
mod subtree;
mod writes;

use crate::ops::{Delta, Txn};
use crate::store::{Log, Resources, Store};
use crate::Limits;
pub use dump::IndexDump;
use fagent_scene_input::{Operation, Result, Snapshot, StorageError};
pub use staged::Staged;
pub use subtree::Subtree;
pub use writes::Written;

/// A loaded project. Reads borrow nodes; writes go through [`ProjectState::begin`] or
/// [`ProjectState::apply`].
pub struct ProjectState {
    store: Store,
    /// The snapshot header (id, slug, name, revision, retention); its vectors are
    /// empty because nodes and resources live in `store`.
    meta: Snapshot,
    /// Revision at load: write stamps are offsets from it.
    loaded: u64,
    limits: Limits,
}

impl ProjectState {
    /// Indexes a snapshot, moving its nodes and resources in. Structural checks only.
    pub fn from_snapshot(snapshot: Snapshot) -> Result<Self> {
        Self::from_snapshot_with(snapshot, Limits::default())
    }

    /// [`Self::from_snapshot`] with explicit limits, kept for later writes.
    pub fn from_snapshot_with(mut snapshot: Snapshot, limits: Limits) -> Result<Self> {
        let nodes = std::mem::take(&mut snapshot.nodes);
        let res = Resources::take(&mut snapshot);
        let mut store = load::build(nodes, limits.max_nodes)?;
        store.res = res;
        Ok(Self {
            store,
            loaded: snapshot.revision,
            meta: snapshot,
            limits,
        })
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// A full snapshot copy: nodes in page-then-preorder order, resources by id.
    pub fn to_snapshot(&self) -> Snapshot {
        let mut snapshot = self.meta.clone();
        snapshot.nodes = self
            .pages()
            .flat_map(|page| self.subtree(&page.id).into_iter().flatten())
            .cloned()
            .collect();
        self.store.res.fill(&mut snapshot);
        snapshot
    }

    pub fn project_id(&self) -> &str {
        &self.meta.project_id
    }

    pub fn revision(&self) -> u64 {
        self.meta.revision
    }

    /// Number of authored nodes.
    pub fn len(&self) -> usize {
        self.store.len()
    }

    pub fn is_empty(&self) -> bool {
        self.store.len() == 0
    }

    /// Approximate bytes held for nodes, indexes and write stamps (not resources).
    pub fn estimated_bytes(&self) -> usize {
        self.store.bytes()
    }

    /// Applies `ops` atomically and keeps them: [`Self::begin`] then commit.
    pub fn apply(&mut self, ops: &[Operation]) -> Result<Delta> {
        Ok(self.begin(ops)?.commit())
    }

    /// Applies `ops` atomically and holds them staged: the result exposes the delta and
    /// the staged state; committing keeps it, aborting or dropping restores exactly the
    /// previous state. On error nothing changes and the error carries the failing
    /// operation index (end-of-batch checks carry none).
    pub fn begin(&mut self, ops: &[Operation]) -> Result<Staged<'_>> {
        let (log, delta) = self.stage(ops)?;
        Ok(Staged::direct(self, log, delta))
    }

    /// Runs the batch; on success the revision is already incremented.
    pub(crate) fn stage(&mut self, ops: &[Operation]) -> Result<(Log, Delta)> {
        if ops.is_empty() {
            return Err(StorageError::validation(
                "operations",
                "A batch needs at least one operation",
            ));
        }
        let revision = self.meta.revision + 1;
        let stamp = u32::try_from(revision - self.loaded).unwrap_or(u32::MAX);
        let mut txn = Txn::new(&mut self.store, self.limits.max_nodes, stamp);
        for (index, op) in ops.iter().enumerate() {
            txn.apply(op).map_err(|e| e.at(index))?;
        }
        txn.finish()?;
        let (log, mut delta) = txn.into_parts();
        delta.revision = revision;
        self.meta.revision = revision;
        Ok((log, delta))
    }

    /// Undoes a staged batch.
    pub(crate) fn unstage(&mut self, log: Log) {
        self.store.rollback(log);
        self.meta.revision -= 1;
    }
}
