//! Per-instance counters of the storage adapter, next to the engine's own.
use fagent_node_memory::MemoryStats;
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};

/// Totals of one storage instance since it was opened.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeMemoryStats {
    /// The engine's counters: loads and admissions (full validation at load, with
    /// failures and time), evictions, expiry, batches by path, and rejections (direct
    /// from the per-operation checks or routed through the reference).
    pub memory: MemoryStats,
    /// Estimated bytes of resident projects.
    pub resident_bytes: u64,
    /// Scoped reads handed a memory view.
    pub read_hits: u64,
    /// Scoped reads served from the database (cold, behind, unknown target, probe
    /// or memory errors, reader errors on a view).
    pub read_fallbacks: u64,
    /// Projects loaded from a read's snapshot because they were cold.
    pub read_loads: u64,
    /// Loads from a read's snapshot because memory was behind the database or of
    /// another incarnation.
    pub read_reloads: u64,
    /// Memory-read failures: panics, rejected loads, reader errors on a view.
    pub read_errors: u64,
    /// Batches and previews that found the project resident at the database's
    /// revision and incarnation.
    pub write_hits: u64,
    /// Batches and previews that (re)loaded the project from the database first.
    pub write_loads: u64,
    /// Prepared batches dropped because persisting them failed; the project was
    /// evicted.
    pub write_aborts: u64,
}

#[derive(Default)]
pub(super) struct Counters {
    pub read_hits: AtomicU64,
    pub read_fallbacks: AtomicU64,
    pub read_loads: AtomicU64,
    pub read_reloads: AtomicU64,
    pub read_errors: AtomicU64,
    pub write_hits: AtomicU64,
    pub write_loads: AtomicU64,
    pub write_aborts: AtomicU64,
}

impl Counters {
    pub fn snapshot(&self, memory: MemoryStats, resident_bytes: usize) -> NodeMemoryStats {
        let get = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        NodeMemoryStats {
            memory,
            resident_bytes: resident_bytes as u64,
            read_hits: get(&self.read_hits),
            read_fallbacks: get(&self.read_fallbacks),
            read_loads: get(&self.read_loads),
            read_reloads: get(&self.read_reloads),
            read_errors: get(&self.read_errors),
            write_hits: get(&self.write_hits),
            write_loads: get(&self.write_loads),
            write_aborts: get(&self.write_aborts),
        }
    }
}

pub(super) fn bump(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::Relaxed);
}
