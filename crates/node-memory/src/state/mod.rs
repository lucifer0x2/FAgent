//! One project held in memory: nodes with live indexes, the snapshot resources
//! (tokens, styles, assets, fonts, notes) and the revision, as one persistent value.
//! Cloning a [`ProjectState`] is O(1); a batch stages on a clone, so the published
//! version doubles as the readers' view (see `view`).
mod dump;
mod load;
#[cfg(test)]
mod lookup_tests;
mod reads;
mod slice;
mod staged;
mod subtree;
mod view;
mod writes;

use crate::ops::{Delta, Rejection, Txn};
use crate::store::{Interned, Resources, Store};
use crate::Limits;
pub use dump::IndexDump;
use fagent_scene_input::{Operation, Result, Snapshot, StorageError};
pub use staged::Staged;
use std::sync::Arc;
pub use subtree::Subtree;
pub use writes::Written;

/// A loaded project. Reads borrow nodes; writes go through [`ProjectState::begin`] or
/// [`ProjectState::apply`].
#[derive(Clone)]
pub struct ProjectState {
    store: Store,
    /// The snapshot header (id, slug, name, revision, retention) with the token,
    /// style, asset and font vectors in id order; no nodes or notes.
    meta: Arc<Snapshot>,
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
        res.fill_resources(&mut snapshot);
        store.res = res;
        Ok(Self {
            store,
            loaded: snapshot.revision,
            meta: Arc::new(snapshot),
            limits,
        })
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// A full snapshot copy: nodes in page-then-preorder order, resources by id.
    pub fn to_snapshot(&self) -> Snapshot {
        let mut snapshot = self.meta.as_ref().clone();
        snapshot.nodes = self
            .pages()
            .flat_map(|page| self.subtree(&page.id).into_iter().flatten())
            .cloned()
            .collect();
        self.store.res.fill(&mut snapshot);
        snapshot
    }

    /// Header and resource vectors (no nodes or notes), shared with readers.
    pub fn metadata(&self) -> &Arc<Snapshot> {
        &self.meta
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

    /// Approximate bytes held for nodes, indexes, interned ids and write stamps (not
    /// resources). Parts shared with other versions count in full.
    pub fn estimated_bytes(&self) -> usize {
        self.store.bytes()
    }

    /// Applies `ops` atomically and keeps them: [`Self::begin`] then commit.
    pub fn apply(&mut self, ops: &[Operation]) -> Result<Delta> {
        Ok(self.begin(ops)?.commit())
    }

    /// Applies `ops` atomically and holds them staged: the result exposes the delta and
    /// the staged state; committing keeps it, aborting or dropping leaves `self` as it
    /// was. On error nothing changes and the error carries the failing operation index
    /// (end-of-batch checks carry none). Tokens are validated once, at the end of the
    /// batch: unlike `Candidate::apply`, a batch whose token set is invalid after one
    /// upsert and repaired by a later one is accepted, and errors after a token upsert
    /// may differ from `Candidate`'s (the engine reports `Candidate`'s for both).
    pub fn begin(&mut self, ops: &[Operation]) -> Result<Staged<'_>> {
        let (next, delta, names) = self.stage(ops).map_err(Rejection::into_error)?;
        Ok(Staged::direct(self, next, delta, names))
    }

    /// Runs the batch on a clone; on success returns the next version (revision
    /// incremented, with its own `meta`), its delta and the names it interned, which
    /// [`Staged`] keeps or discards. `self` never changes. A rejected batch removes
    /// its names again (see `store::Interned`). An empty batch is the engine's own
    /// input error, reported as is.
    pub(crate) fn stage(
        &self,
        ops: &[Operation],
    ) -> std::result::Result<(ProjectState, Delta, Interned), Rejection> {
        if ops.is_empty() {
            return Err(StorageError::validation(
                "operations",
                "A batch needs at least one operation",
            )
            .into());
        }
        let names = self.store.open_names();
        let mut next = self.clone();
        match next.run(ops) {
            Ok(delta) => Ok((next, delta, names)),
            Err(rejection) => {
                // `Txn` only borrowed the clone: nothing else can hold its keys.
                drop(next);
                names.discard(true);
                Err(rejection)
            }
        }
    }

    /// Applies the batch to this fresh clone of the base and moves it to the next
    /// revision.
    fn run(&mut self, ops: &[Operation]) -> std::result::Result<Delta, Rejection> {
        let revision = self.meta.revision + 1;
        let stamp = u32::try_from(revision - self.loaded).unwrap_or(u32::MAX);
        let mut txn = Txn::new(&mut self.store, self.limits.max_nodes, stamp);
        for (index, op) in ops.iter().enumerate() {
            txn.apply(op).map_err(|e| e.at(index))?;
        }
        txn.finish()?;
        let mut delta = txn.into_delta();
        delta.revision = revision;
        // Always a copy: `meta` is shared with the base, so the staged version owns
        // its `meta` from here on (`Staged` relies on that to see it is unreachable).
        let meta = Arc::make_mut(&mut self.meta);
        meta.revision = revision;
        if !delta.tokens.is_empty() || !delta.styles.is_empty() {
            self.store.res.fill_resources(meta);
        }
        Ok(delta)
    }
}
