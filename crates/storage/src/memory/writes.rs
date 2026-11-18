//! The resident project a batch or preview stages in.
//!
//! Callers hold the backend's write serialization (the SQLite writer lock, the
//! PostgreSQL project row lock) or a repeatable-read transaction for a preview, and
//! pass the database identity and revision they read under it. A resident load is
//! used only when its tag has that incarnation and its settled view that revision;
//! a load of the same incarnation that is already past it is never replaced by an
//! older snapshot. Otherwise the project is evicted and loaded again from the
//! database.
use super::identity::Tag;
use super::stats::bump;
use super::Memory;
use crate::{Result, Snapshot, SnapshotCacheIdentity, StorageError};
use fagent_node_memory::{HotProject, MemoryConfig, NodeMemory, SystemClock};
use std::cmp::Ordering;
use std::sync::Arc;

/// Tries before a load that keeps losing to concurrent readers gives up.
const LOAD_TRIES: usize = 3;

/// What memory holds for one project, against a database revision.
enum Resident {
    /// Tagged with the incarnation and settled at the revision.
    Fresh(HotProject),
    /// Tagged with the incarnation and published past the revision.
    Ahead,
    /// Cold, untagged, of another incarnation or behind.
    Stale,
}

impl Memory {
    /// The project to stage a batch in, at `revision` of the database, under the
    /// backend's write serialization.
    ///
    /// `identity` is `None` when the backend cannot name the incarnation (no file
    /// stamp): the project is then loaded every time and left untagged. A load the
    /// engine rejects is remembered for that identity and returns
    /// `VALIDATION_FAILED` to every later write until the identity changes.
    pub(crate) fn for_write(
        &self,
        project_id: &str,
        identity: Option<&SnapshotCacheIdentity>,
        revision: u64,
        load: impl FnMut() -> Result<Snapshot>,
    ) -> Result<HotProject> {
        // Under the write serialization nothing of this incarnation publishes past
        // the database: a resident load ahead of it is reloaded.
        let staged = self.stage(project_id, identity, revision, false, load)?;
        staged.ok_or_else(|| StorageError::new("STORAGE_FAILED", "Project could not be staged"))
    }

    /// The project a preview at `revision` stages in, or `None` when memory already
    /// published a later revision of the same incarnation: the preview's snapshot is
    /// older than a commit that landed after it, and the newer copy stays resident.
    pub(crate) fn for_preview(
        &self,
        project_id: &str,
        identity: &SnapshotCacheIdentity,
        revision: u64,
        load: impl FnMut() -> Result<Snapshot>,
    ) -> Result<Option<HotProject>> {
        self.stage(project_id, Some(identity), revision, true, load)
    }

    /// A private load of `load`'s snapshot that never becomes resident, for a
    /// preview whose snapshot every resident copy has moved past.
    pub(crate) fn detached(&self, load: impl FnOnce() -> Result<Snapshot>) -> Result<HotProject> {
        let engine = NodeMemory::new(MemoryConfig::default(), Arc::new(SystemClock::new()));
        engine.load(load()?).map_err(invalid)
    }

    fn stage(
        &self,
        project_id: &str,
        identity: Option<&SnapshotCacheIdentity>,
        revision: u64,
        keep_ahead: bool,
        mut load: impl FnMut() -> Result<Snapshot>,
    ) -> Result<Option<HotProject>> {
        let inner = &self.inner;
        inner.engine.maybe_expire();
        if let Some(identity) = identity {
            match self.resident(project_id, identity, revision) {
                Resident::Fresh(project) => {
                    bump(&inner.counters.write_hits);
                    return Ok(Some(project));
                }
                Resident::Ahead if keep_ahead => return Ok(None),
                Resident::Ahead | Resident::Stale => {}
            }
            if let Some(error) = inner.tags.failed_error(identity) {
                return Err(error);
            }
        }
        bump(&inner.counters.write_loads);
        for attempt in 0..LOAD_TRIES {
            // After a lost race, use what the winner loaded when it fits.
            if let (Some(identity), true) = (identity, attempt > 0) {
                match self.resident(project_id, identity, revision) {
                    Resident::Fresh(project) => return Ok(Some(project)),
                    Resident::Ahead if keep_ahead => return Ok(None),
                    Resident::Ahead | Resident::Stale => {}
                }
            }
            self.evict(project_id);
            let snapshot = load()?;
            match crate::bench_phase::time("memory.load", || inner.engine.load(snapshot)) {
                Ok(project) => {
                    if let Some(identity) = identity {
                        let tag = Tag::loaded(project.load_id(), identity);
                        inner.tags.insert(&inner.engine, project_id.into(), tag);
                    }
                    return Ok(Some(project));
                }
                // A reader loaded it between the eviction and this load.
                Err(error) if error.code == "PROJECT_EXISTS" => continue,
                Err(error) => {
                    let error = invalid(error);
                    if let Some(identity) = identity {
                        let tag = Tag::failed_with(identity, error.clone());
                        inner.tags.insert(&inner.engine, project_id.into(), tag);
                    }
                    return Err(error);
                }
            }
        }
        Err(StorageError::new(
            "STORAGE_FAILED",
            "Project could not be loaded into memory",
        ))
    }

    /// The resident load against `revision` of `identity`'s incarnation. The settled
    /// view waits for a batch this process staged: PostgreSQL releases the row lock
    /// when the transaction commits, before the batch publishes.
    fn resident(
        &self,
        project_id: &str,
        identity: &SnapshotCacheIdentity,
        revision: u64,
    ) -> Resident {
        let inner = &self.inner;
        let Some(project) = inner.engine.project(project_id) else {
            return Resident::Stale;
        };
        if !inner.tags.tagged(project_id, project.load_id(), identity) {
            return Resident::Stale;
        }
        let published = project.settled_view(revision).metadata().revision;
        match published.cmp(&revision) {
            Ordering::Equal => Resident::Fresh(project),
            Ordering::Greater => Resident::Ahead,
            Ordering::Less => Resident::Stale,
        }
    }

    /// After the database committed and `project` published the batch, still
    /// under the write serialization: the resident load now matches `identity`
    /// (the new revision, and for SQLite the file stamp after the write).
    pub(crate) fn committed(&self, project: &HotProject, identity: Option<&SnapshotCacheIdentity>) {
        self.inner
            .tags
            .retag(project.id(), project.load_id(), identity);
    }

    /// Persisting a prepared batch failed. Drop the prepared batch first (it rolls
    /// back); the database may still have committed, so the project leaves memory.
    pub(crate) fn write_failed(&self, project_id: &str) {
        bump(&self.inner.counters.write_aborts);
        self.evict(project_id);
    }
}

/// Stored rows the engine cannot load are invalid project data.
fn invalid(error: StorageError) -> StorageError {
    if error.code == "VALIDATION_FAILED" {
        return error;
    }
    let mut out = StorageError::new("VALIDATION_FAILED", &error.message);
    out.field = error.field;
    out
}
