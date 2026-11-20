//! The storage adapter around [`NodeMemory`]: always on, no modes.
//!
//! The engine knows nothing about databases. This adapter keeps, per project, the
//! database incarnation a resident load came from ([`identity`]), serves scoped and
//! full-snapshot reads of the latest revision from the engine's published views
//! ([`reads`]) and counts what it did ([`stats`]). Batches and previews stage in the resident
//! project ([`writes`]) after the backend probed the database identity under its
//! write lock; both backends begin the batch and build its result and history envelope
//! through [`stage`]; strict page guards run on the engine views ([`guard`]). Writes outside
//! the engine (asset and font imports, privacy updates, bundle opens, a failed
//! persist) call [`Memory::evict`]; the next access reloads the project.
pub(crate) mod guard;
#[cfg(test)]
mod guard_tests;
mod identity;
mod reads;
#[cfg(test)]
mod reads_tests;
pub(crate) mod stage;
mod stats;
mod writes;
#[cfg(test)]
mod writes_tests;

use crate::{SharedSnapshot, SnapshotCacheIdentity};
use fagent_node_memory::{MemoryConfig, NodeMemory, SystemClock};
use identity::Tags;
pub use stats::NodeMemoryStats;
use stats::{bump, Counters};
use std::sync::Arc;

/// Cheap to clone; clones share the engine, tags and counters.
#[derive(Clone)]
pub(crate) struct Memory {
    inner: Arc<Inner>,
}

struct Inner {
    engine: NodeMemory,
    tags: Tags,
    counters: Counters,
}

impl Default for Memory {
    fn default() -> Self {
        let engine = NodeMemory::new(MemoryConfig::default(), Arc::new(SystemClock::new()));
        Self {
            inner: Arc::new(Inner {
                engine,
                tags: Tags::default(),
                counters: Counters::default(),
            }),
        }
    }
}

impl Memory {
    /// Whether a tagged load of `project` is resident. Cache policy only, never
    /// authority to serve data: reads still require a fresh database identity probe.
    #[cfg(test)]
    pub fn owns_project(&self, project: &str) -> bool {
        let inner = &self.inner;
        inner
            .engine
            .resident(project)
            .is_some_and(|load| inner.tags.tags_load(project, load))
    }

    /// Drops a project after a write the engine did not stage.
    pub fn evict(&self, project: &str) {
        self.inner.tags.remove(project);
        self.inner.engine.evict(project);
    }

    /// Records that a reader failed on a memory view and the read was served again
    /// from the database.
    pub fn read_failed(&self) {
        let counters = &self.inner.counters;
        bump(&counters.read_errors);
        bump(&counters.read_fallbacks);
    }

    pub fn stats(&self) -> NodeMemoryStats {
        self.inner
            .counters
            .snapshot(self.inner.engine.stats(), self.inner.engine.total_bytes())
    }
}

/// The snapshot one scoped read runs on.
pub struct ScopedRead {
    pub shared: SharedSnapshot,
    /// An immutable whole-project memory view rather than a database snapshot.
    pub from_memory: bool,
}

impl ScopedRead {
    pub(crate) fn database(shared: SharedSnapshot) -> Self {
        Self {
            shared,
            from_memory: false,
        }
    }
}

/// The database path for one project: its shared snapshot and the identity of that
/// read.
pub(crate) type Loaded = (SharedSnapshot, Option<SnapshotCacheIdentity>);
