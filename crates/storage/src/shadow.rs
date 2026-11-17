//! Opt-in shadow of committed batches in `fagent-node-memory` (Task28 I1), and
//! scoped reads served from it (Task28 I2).
//!
//! After a batch commits, both engines hand its base snapshot, operations, result
//! and persisted rows to [`Shadow::observe`], which applies the operations to the
//! in-memory project and compares the outcome with the database. Writes outside
//! batches call [`Shadow::evict`]; the next commit reloads from its base snapshot.
//! The shadow only counts, logs and evicts: its failures never reach a request,
//! except in the test-only strict mode, which panics on a mismatch. With reads on,
//! [`Shadow::scoped_read`] serves scoped reads from the same registry.
mod compare;
mod incremental;
mod mirror;
mod read_mode;
mod reads;
#[cfg(test)]
mod reads_tests;
mod rows;
mod stats;
#[cfg(test)]
mod tests;
mod write_mode;
mod writes;

use crate::{BatchResult, Operation, Snapshot, SnapshotCacheIdentity};
use mirror::{Failure, Mirror, Outcome};
pub(crate) use read_mode::Loaded;
pub use read_mode::{ReadsMode, ScopedRead, NODE_MEMORY_READS_ENV};
use reads::Reads;
pub(crate) use rows::{Kind, Rows};
use stats::{add_micros, bump, COUNTERS};
pub use stats::{node_memory_read_failed, node_memory_shadow_stats, NodeMemoryShadowStats};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
pub use write_mode::{WritesMode, NODE_MEMORY_WRITES_ENV};
pub(crate) use writes::{rows as memory_rows, MemoryBase, MemoryRows};

/// Environment switch: `1`/`on`/`true` enables the shadow, `strict` also compares
/// the whole project after every batch and panics on a mismatch (tests only).
pub const NODE_MEMORY_SHADOW_ENV: &str = "FAGENT_NODE_MEMORY_SHADOW";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShadowMode {
    #[default]
    Off,
    On,
    Strict,
}

impl ShadowMode {
    pub fn from_env() -> Self {
        Self::parse(std::env::var(NODE_MEMORY_SHADOW_ENV).ok().as_deref())
    }

    fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("1" | "on" | "true") => Self::On,
            Some("strict") => Self::Strict,
            _ => Self::Off,
        }
    }
}

/// One committed batch as the database stored it.
pub(crate) struct Commit<'a> {
    /// The project at the batch's base revision, as the writer loaded it.
    pub before: &'a Snapshot,
    pub operations: &'a [Operation],
    pub result: &'a BatchResult,
    pub rows: &'a Rows,
    /// The incarnation `before` was read from, to tag a project this commit loads.
    pub identity: Option<&'a SnapshotCacheIdentity>,
    /// Applies asset readiness as reads see it to a copy of `before` before loading.
    pub ready: Option<&'a dyn Fn(&mut Snapshot)>,
}

/// Cheap to clone; disabled unless constructed with a mode other than `Off`.
#[derive(Clone, Default)]
pub(crate) struct Shadow {
    mirror: Option<Arc<Mirror>>,
    reads: Option<Arc<Reads>>,
    /// Task28 I3: commits staged in memory; `true` in strict mode.
    writes: Option<bool>,
}

impl Shadow {
    /// Exclude duplicate legacy retention whenever a tagged reader-owned project
    /// is resident. This is cache policy only, never authority to serve data:
    /// actual reads still require the full fresh database identity probe.
    pub fn owns_project(&self, project: &str) -> bool {
        let (Some(reads), Some(mirror)) = (&self.reads, &self.mirror) else {
            return false;
        };
        mirror
            .registry
            .get(project)
            .is_some_and(|handle| reads.tags_handle(&handle))
    }
    /// Claim transition cleanup once, stored on the existing incarnation tag.
    /// Retagging the same handle preserves it; reload/eviction gets a new claim.
    pub fn claim_cache_cleanup(&self, project: &str) -> bool {
        let (Some(reads), Some(mirror)) = (&self.reads, &self.mirror) else {
            return false;
        };
        mirror
            .registry
            .get(project)
            .is_some_and(|handle| reads.claim_cache_cleanup(&handle))
    }
    /// Writes on imply reads on, and reads on imply the mirror on: it is what keeps
    /// memory current when a commit takes today's path.
    pub fn new(mode: ShadowMode, reads: ReadsMode, writes: WritesMode) -> Self {
        let reads = match (reads, writes) {
            (ReadsMode::Off, WritesMode::On | WritesMode::Strict) => ReadsMode::On,
            (reads, _) => reads,
        };
        let mode = match (mode, reads) {
            (ShadowMode::Off, ReadsMode::On | ReadsMode::Strict) => ShadowMode::On,
            (mode, _) => mode,
        };
        let mirror = match mode {
            ShadowMode::Off => None,
            ShadowMode::On => Some(Arc::new(Mirror::new(false))),
            ShadowMode::Strict => Some(Arc::new(Mirror::new(true))),
        };
        Self {
            mirror,
            reads: Reads::new(reads).map(Arc::new),
            writes: (writes != WritesMode::Off).then_some(writes == WritesMode::Strict),
        }
    }

    pub fn enabled(&self) -> bool {
        self.mirror.is_some()
    }

    /// Mirrors one committed batch. Call after the database commit, before the
    /// project's next batch can commit where the engine allows it.
    pub fn observe(&self, commit: Commit<'_>) {
        let Some(mirror) = &self.mirror else {
            return;
        };
        crate::bench_phase::time("shadow", || {
            let start = std::time::Instant::now();
            let outcome = catch_unwind(AssertUnwindSafe(|| mirror.run(&commit)))
                .unwrap_or_else(|_| Err(Failure::Error("panic".into())));
            match outcome {
                Ok(Outcome::Matched(loaded)) => {
                    bump(&COUNTERS.matched);
                    if let (Some(handle), Some(identity)) = (loaded, commit.identity) {
                        self.tag(&handle, identity);
                    }
                }
                Ok(Outcome::Skipped) => bump(&COUNTERS.skipped),
                Err(failure) => mirror.fail(&commit, failure),
            }
            mirror.expire();
            add_micros(&COUNTERS.observe_micros, start);
        });
    }

    /// Drops a project after a write outside batches.
    pub fn evict(&self, project_id: &str) {
        if let Some(mirror) = &self.mirror {
            if mirror.evict(project_id) {
                bump(&COUNTERS.evictions);
            }
        }
    }

    /// The snapshot a scoped read of `target` runs on: a published view when it is
    /// at the revision `probe` reports, else today's `load`, which then (re)loads
    /// memory. With reads off this is `load` alone; `load`'s errors are today's.
    pub fn scoped_read(
        &self,
        target: &str,
        probe: impl FnOnce() -> crate::Result<Option<crate::SnapshotCacheIdentity>>,
        load: impl FnOnce() -> crate::Result<Loaded>,
    ) -> crate::Result<ScopedRead> {
        match (&self.reads, &self.mirror) {
            (Some(reads), Some(mirror)) => reads.read(mirror, target, probe, load),
            _ => Ok(ScopedRead::database(load()?.0)),
        }
    }
}
