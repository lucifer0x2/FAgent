//! Per-instance counters. Relaxed atomics: they are diagnostics, never control flow.
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};

/// A copy of the counters of one [`crate::NodeMemory`] at one moment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct MemoryStats {
    /// Successful `load` calls.
    pub loads: u64,
    /// `load` calls that failed validation (a lost race is not a failure).
    pub load_failures: u64,
    /// Time spent in successful loads, in `Clock` microseconds.
    pub load_micros: u64,
    /// Projects dropped by `evict`.
    pub evictions: u64,
    /// Projects dropped by the TTL.
    pub expired: u64,
    /// Projects dropped by the byte budget.
    pub over_budget: u64,
    /// Batches kept.
    pub commits: u64,
    /// Prepared batches dropped without commit.
    pub rollbacks: u64,
    /// Batches validated on the delta path.
    pub local: u64,
    /// Batches validated by the reference path.
    pub full: u64,
    /// Batches rejected by validation.
    pub rejected: u64,
    /// Batches where the engine and the reference validator disagreed.
    pub divergences: u64,
}

#[derive(Default)]
pub(crate) struct Stats {
    pub(crate) loads: AtomicU64,
    pub(crate) load_failures: AtomicU64,
    pub(crate) load_micros: AtomicU64,
    pub(crate) evictions: AtomicU64,
    pub(crate) expired: AtomicU64,
    pub(crate) over_budget: AtomicU64,
    pub(crate) commits: AtomicU64,
    pub(crate) rollbacks: AtomicU64,
    pub(crate) local: AtomicU64,
    pub(crate) full: AtomicU64,
    pub(crate) rejected: AtomicU64,
    pub(crate) divergences: AtomicU64,
}

impl Stats {
    pub(crate) fn bump(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> MemoryStats {
        let get = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        MemoryStats {
            loads: get(&self.loads),
            load_failures: get(&self.load_failures),
            load_micros: get(&self.load_micros),
            evictions: get(&self.evictions),
            expired: get(&self.expired),
            over_budget: get(&self.over_budget),
            commits: get(&self.commits),
            rollbacks: get(&self.rollbacks),
            local: get(&self.local),
            full: get(&self.full),
            rejected: get(&self.rejected),
            divergences: get(&self.divergences),
        }
    }
}
