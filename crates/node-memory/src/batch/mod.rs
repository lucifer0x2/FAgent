//! One batch against a resident project: apply it under the project's write lock, validate
//! it, and hand an adapter everything it needs to persist and publish it.
//!
//! [`HotProject::begin`] stages the batch through `ProjectState::stage`: `Candidate::apply`'s
//! per-operation checks in `Txn` on the working state (token writes included), the
//! node cap, then the end-of-batch delta rules (`validate::delta`) over the base and
//! staged states. Every rejection is the public error as is: per-operation errors carry
//! their operation index, end-of-batch errors none. No snapshot, `Candidate` or
//! whole-project pass runs on this path.
mod footprint;
mod rows;

use crate::engine::{HotProject, Stats};
use crate::{Delta, Staged};
use fagent_scene_input::read_scope::ReadView;
use fagent_scene_input::{Operation, Result, StorageError};
use std::cell::OnceCell;
use std::collections::BTreeSet;
use std::sync::Arc;

pub use rows::Rows;

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
            Err(rejection) => return Err(self.reject(rejection.into_error())),
        };
        let changed = staged.delta().changed_ids();
        let after = staged.preview();
        Ok(Begin::Ready(Prepared {
            staged: Some(staged),
            before,
            after,
            changed,
            footprint: OnceCell::new(),
            stats: self.stats.clone(),
        }))
    }

    fn reject(&self, error: StorageError) -> StorageError {
        Stats::bump(&self.stats.rejected);
        error
    }
}
