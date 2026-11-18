//! One batch against a resident project: apply it under the project's write lock, validate
//! it, and hand an adapter everything it needs to persist and publish it.
//!
//! [`HotProject::begin`] stages the batch (per-operation checks in the lib's `Txn`), then
//! validates the result:
//!
//! - an operation class the classifier supports is checked on its delta only
//!   (`validate_delta`); a batch the delta validator cannot decide runs the reference;
//! - every other class runs the reference on the staged batch.
//!
//! The reference is `Candidate::apply` per operation, link pruning and `validate_after`
//! over the base view, all in memory. `Txn` runs `Candidate::apply`'s per-operation
//! checks in its order on the working state (token writes included), and the
//! end-of-batch node cap first, as full validation does, so each of its rejections is
//! the public error as is (`MemoryStats::rejected_direct`). If the reference produces
//! other changed ids than the lib, the engine and the validator disagree: the project
//! is flagged as diverged and `STORAGE_FAILED` is returned.
//!
//! Token batches are never local (`memory::supports` rejects them).
mod footprint;
mod reference;
mod rows;

use crate::engine::{HotProject, Stats};
use crate::{Delta, Staged};
use fagent_scene_input::read_scope::ReadView;
use fagent_scene_input::validate::memory;
use fagent_scene_input::{Operation, Result, StorageError};
use std::cell::OnceCell;
use std::collections::BTreeSet;
use std::sync::Arc;

pub use rows::Rows;

/// How a batch was validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Path {
    /// On its delta and dependency closure.
    Local,
    /// By the reference validator over the whole project.
    Full,
}

/// The outcome of [`HotProject::begin`].
#[must_use = "a prepared batch is rolled back when dropped"]
// One value per batch, moved once to the caller: boxing it would only add an allocation.
#[allow(clippy::large_enum_variant)]
pub enum Begin<'a> {
    Ready(Prepared<'a>),
    /// The project is not at the expected revision (another writer got there first, or
    /// the adapter's view of it is out of date): reload and retry.
    Stale,
}

/// A validated batch, staged under the project's write lock: readers of the published
/// view are not affected, other writers wait. [`Prepared::commit`] publishes it after
/// the adapter persisted [`Prepared::rows`]; dropping it rolls it back exactly.
#[must_use = "a prepared batch is rolled back when dropped; call commit() to keep it"]
pub struct Prepared<'a> {
    // Drops before `staged`: a rolled-back batch discards its interned names only
    // when no view of it survives (see `Staged`).
    after: Arc<ReadView>,
    staged: Option<Staged<'a>>,
    before: Arc<ReadView>,
    path: Path,
    changed: Vec<String>,
    footprint: OnceCell<BTreeSet<String>>,
    stats: Arc<Stats>,
}

impl<'a> Prepared<'a> {
    /// The revision the batch was applied to.
    pub fn base_revision(&self) -> u64 {
        self.before.metadata().revision
    }

    /// The revision the batch creates.
    pub fn revision(&self) -> u64 {
        self.after.metadata().revision
    }

    /// Every id the batch touched, as `Candidate` reports them (`BatchResult::changed_ids`),
    /// sorted. Includes ids whose row did not change in the end.
    pub fn changed_ids(&self) -> &[String] {
        &self.changed
    }

    /// The rows that differ from the base revision.
    pub fn rows(&self) -> Rows {
        let staged = self.staged.as_ref().expect("batch is staged");
        rows::build(staged.state(), staged.delta(), &self.before)
    }

    /// History footprint keys: `node:`, `page:`, `note:`, `token:`, `style:` or `*`.
    pub fn footprint(&self) -> &BTreeSet<String> {
        self.footprint.get_or_init(|| {
            let staged = self.staged.as_ref().expect("batch is staged");
            footprint::build(&self.before, &self.after, staged.delta(), &self.changed)
        })
    }

    /// The published view the batch started from.
    pub fn before(&self) -> &Arc<ReadView> {
        &self.before
    }

    /// The staged successor; published only by [`Prepared::commit`].
    pub fn after(&self) -> &Arc<ReadView> {
        &self.after
    }

    pub fn path(&self) -> Path {
        self.path
    }

    /// Publishes the batch and returns its delta.
    pub fn commit(mut self) -> Delta {
        Stats::bump(&self.stats.commits);
        self.staged.take().expect("batch is staged").commit()
    }
}

impl Drop for Prepared<'_> {
    fn drop(&mut self) {
        if self.staged.is_some() {
            Stats::bump(&self.stats.rollbacks);
        }
    }
}

impl HotProject {
    /// Applies `ops` atomically on top of `expected_revision` and validates them.
    ///
    /// `Ok(Begin::Stale)` when the project is at another revision. A rejected batch
    /// leaves the project untouched and returns the public error, with the failing
    /// operation index for per-operation errors.
    pub fn begin(&self, expected_revision: u64, ops: &[Operation]) -> Result<Begin<'_>> {
        let Some((before, staged)) = self.handle.begin_at(expected_revision, ops) else {
            return Ok(Begin::Stale);
        };
        let mut staged = match staged {
            Ok(staged) => staged,
            Err(rejection) => {
                Stats::bump(&self.stats.rejected_direct);
                return Err(self.reject(rejection.into_error()));
            }
        };
        let changed = staged.delta().changed_ids();
        let after = staged.preview();
        let local = memory::validate_delta(&before, &after, ops, &changed);
        let path = match local {
            Err(error) => return Err(self.reject(error)),
            Ok(Some(_)) => Path::Local,
            Ok(None) => match reference::run(&before, ops) {
                Err(error) => return Err(self.reject(error)),
                Ok(ids) if ids != changed => return Err(self.diverge("changed ids")),
                Ok(_) => Path::Full,
            },
        };
        Stats::bump(match path {
            Path::Local => &self.stats.local,
            Path::Full => &self.stats.full,
        });
        Ok(Begin::Ready(Prepared {
            staged: Some(staged),
            before,
            after,
            path,
            changed,
            footprint: OnceCell::new(),
            stats: self.stats.clone(),
        }))
    }

    fn reject(&self, error: StorageError) -> StorageError {
        Stats::bump(&self.stats.rejected);
        error
    }

    fn diverge(&self, what: &str) -> StorageError {
        Stats::bump(&self.stats.divergences);
        self.handle.mark_diverged();
        eprintln!(
            "{}",
            serde_json::json!({
                "event": "node_memory_divergence",
                "project": self.id(),
                "what": what,
            })
        );
        StorageError::new(
            "STORAGE_FAILED",
            "In-memory project disagrees with the reference validator",
        )
    }
}
