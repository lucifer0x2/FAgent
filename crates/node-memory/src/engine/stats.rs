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
    /// Time spent in successful loads, in `Clock` microseconds (admission excluded).
    pub load_micros: u64,
    /// Full validations of a loaded revision (one per successful load).
    pub admissions: u64,
    /// Admissions that found the stored revision invalid (it is still served).
    pub admission_failures: u64,
    /// Time spent in admissions, in `Clock` microseconds.
    pub admission_micros: u64,
    /// Projects dropped by `evict`.
    pub evictions: u64,
    /// Projects dropped by the TTL.
    pub expired: u64,
    /// Projects dropped by the byte budget.
    pub over_budget: u64,
    /// Projects dropped to compact the ids of deleted nodes.
    pub sparse: u64,
    /// Batches kept.
    pub commits: u64,
    /// Prepared batches dropped without commit.
    pub rollbacks: u64,
    /// Batches rejected by validation (per-operation or end-of-batch).
    pub rejected: u64,
}

#[derive(Default)]
pub(crate) struct Stats {
    pub(crate) loads: AtomicU64,
    pub(crate) load_failures: AtomicU64,
    pub(crate) load_micros: AtomicU64,
    pub(crate) admissions: AtomicU64,
    pub(crate) admission_failures: AtomicU64,
    pub(crate) admission_micros: AtomicU64,
    pub(crate) evictions: AtomicU64,
    pub(crate) expired: AtomicU64,
    pub(crate) over_budget: AtomicU64,
    pub(crate) sparse: AtomicU64,
    pub(crate) commits: AtomicU64,
    pub(crate) rollbacks: AtomicU64,
    pub(crate) rejected: AtomicU64,
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
            admissions: get(&self.admissions),
            admission_failures: get(&self.admission_failures),
            admission_micros: get(&self.admission_micros),
            evictions: get(&self.evictions),
            expired: get(&self.expired),
            over_budget: get(&self.over_budget),
            sparse: get(&self.sparse),
            commits: get(&self.commits),
            rollbacks: get(&self.rollbacks),
            rejected: get(&self.rejected),
        }
    }
}
