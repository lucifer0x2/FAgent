//! The resident project a batch or preview stages in.
//!
//! Callers hold the backend's write serialization (the SQLite writer lock, the
//! PostgreSQL project row lock) or a repeatable-read transaction for a preview, and
//! pass the database identity and revision they read under it. A resident load is
//! used only when its tag has that incarnation and its published view that
//! revision; otherwise the project is evicted and loaded again from the database.
use super::identity::Tag;
use super::stats::bump;
use super::Memory;
use crate::{Result, Snapshot, SnapshotCacheIdentity, StorageError};
use fagent_node_memory::HotProject;

/// Tries before a load that keeps losing to concurrent readers gives up.
const LOAD_TRIES: usize = 3;

impl Memory {
    /// The project to stage a batch in, at `revision` of the database.
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
        mut load: impl FnMut() -> Result<Snapshot>,
    ) -> Result<HotProject> {
        let inner = &self.inner;
        inner.engine.maybe_expire();
        if let Some(identity) = identity {
            if let Some(project) = inner.engine.project(project_id) {
                if inner.tags.tagged(project_id, project.load_id(), identity)
                    && project.view().metadata().revision == revision
                {
                    bump(&inner.counters.write_hits);
                    return Ok(project);
                }
            }
            if let Some(error) = inner.tags.failed_error(identity) {
                return Err(error);
            }
        }
        bump(&inner.counters.write_loads);
        for _ in 0..LOAD_TRIES {
            self.evict(project_id);
            let snapshot = load()?;
            match crate::bench_phase::time("memory.load", || inner.engine.load(snapshot)) {
                Ok(project) => {
                    if let Some(identity) = identity {
                        let tag = Tag::loaded(project.load_id(), identity);
                        inner.tags.insert(&inner.engine, project_id.into(), tag);
                    }
                    return Ok(project);
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

    /// After the database committed and `project` published the batch, still
    /// under the write serialization: the resident load now matches `identity`
    /// (the new revision, and for SQLite the file stamp after the write).
    pub(crate) fn committed(&self, project: &HotProject, identity: Option<&SnapshotCacheIdentity>) {
        self.inner
            .tags
            .retag(project.id(), project.load_id(), identity);
    }

    /// Whether the resident project is past `revision`: a preview on an older
    /// database snapshot would reload it backwards.
    pub(crate) fn ahead_of(&self, project_id: &str, revision: u64) -> bool {
        self.inner
            .engine
            .project(project_id)
            .is_some_and(|project| project.view().metadata().revision > revision)
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
