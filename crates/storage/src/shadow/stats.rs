//! Process-wide shadow and memory-read counters, for tests and diagnostics.
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};

/// Totals since process start, summed over every storage instance with the shadow on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeMemoryShadowStats {
    /// Commits applied in memory and found identical.
    pub matched: u64,
    /// Commits whose in-memory result differed (the project was evicted).
    pub mismatches: u64,
    /// Projects loaded from a commit's base snapshot (first touch or after eviction).
    pub loads: u64,
    /// Loads because memory lagged behind the database's base revision.
    pub resyncs: u64,
    /// Commits skipped because memory was already past their base revision.
    pub skipped: u64,
    /// Explicit evictions after writes outside batches.
    pub evictions: u64,
    /// Shadow failures (load errors, panics); the project was evicted.
    pub errors: u64,
    /// Microseconds spent mirroring commits, loads included.
    pub observe_micros: u64,
    /// Microseconds of those spent loading projects from base snapshots.
    pub load_micros: u64,
    /// Scoped reads handed a slice of memory (Task28 I2).
    pub read_hits: u64,
    /// Scoped reads served by today's path while reads were on (cold, behind,
    /// unknown target, probe or memory errors, reader errors on a slice).
    pub read_fallbacks: u64,
    /// Projects loaded into memory from a read's snapshot because they were cold.
    pub read_loads: u64,
    /// Loads from a read's snapshot because memory was behind the database, of
    /// another incarnation, or loaded by the mirror without one.
    pub read_reloads: u64,
    /// Memory-read failures: panics, rejected loads, reader errors on a slice.
    pub read_errors: u64,
    /// Commits staged and persisted from memory (Task28 I3).
    pub write_hits: u64,
    /// Commits through today's path while writes were on (memory cold, behind,
    /// untagged or of another incarnation).
    pub write_misses: u64,
    /// Lib rejections routed to the reference path. I4 stages before property
    /// validation, so this includes invalid requests the reference also rejects.
    pub write_fallbacks: u64,
    /// Staged batches rolled back because persisting them failed.
    pub write_aborts: u64,
    /// Batches accepted by the exact-delta validator (Task28 I4).
    pub write_incremental: u64,
}

pub(super) struct Counters {
    pub matched: AtomicU64,
    pub mismatches: AtomicU64,
    pub loads: AtomicU64,
    pub resyncs: AtomicU64,
    pub skipped: AtomicU64,
    pub evictions: AtomicU64,
    pub errors: AtomicU64,
    pub observe_micros: AtomicU64,
    pub load_micros: AtomicU64,
    pub read_hits: AtomicU64,
    pub read_fallbacks: AtomicU64,
    pub read_loads: AtomicU64,
    pub read_reloads: AtomicU64,
    pub read_errors: AtomicU64,
    pub write_hits: AtomicU64,
    pub write_misses: AtomicU64,
    pub write_fallbacks: AtomicU64,
    pub write_aborts: AtomicU64,
    pub write_incremental: AtomicU64,
}

pub(super) static COUNTERS: Counters = Counters {
    matched: AtomicU64::new(0),
    mismatches: AtomicU64::new(0),
    loads: AtomicU64::new(0),
    resyncs: AtomicU64::new(0),
    skipped: AtomicU64::new(0),
    evictions: AtomicU64::new(0),
    errors: AtomicU64::new(0),
    observe_micros: AtomicU64::new(0),
    load_micros: AtomicU64::new(0),
    read_hits: AtomicU64::new(0),
    read_fallbacks: AtomicU64::new(0),
    read_loads: AtomicU64::new(0),
    read_reloads: AtomicU64::new(0),
    read_errors: AtomicU64::new(0),
    write_hits: AtomicU64::new(0),
    write_misses: AtomicU64::new(0),
    write_fallbacks: AtomicU64::new(0),
    write_aborts: AtomicU64::new(0),
    write_incremental: AtomicU64::new(0),
};

pub(super) fn bump(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::Relaxed);
}

pub(super) fn add_micros(counter: &AtomicU64, since: std::time::Instant) {
    counter.fetch_add(since.elapsed().as_micros() as u64, Ordering::Relaxed);
}

/// Records that a reader failed on a memory slice and the read was served again
/// by today's path.
pub fn node_memory_read_failed() {
    bump(&COUNTERS.read_errors);
    bump(&COUNTERS.read_fallbacks);
}

/// Current process-wide totals.
pub fn node_memory_shadow_stats() -> NodeMemoryShadowStats {
    let c = &COUNTERS;
    let get = |a: &AtomicU64| a.load(Ordering::Relaxed);
    NodeMemoryShadowStats {
        matched: get(&c.matched),
        mismatches: get(&c.mismatches),
        loads: get(&c.loads),
        resyncs: get(&c.resyncs),
        skipped: get(&c.skipped),
        evictions: get(&c.evictions),
        errors: get(&c.errors),
        observe_micros: get(&c.observe_micros),
        load_micros: get(&c.load_micros),
        read_hits: get(&c.read_hits),
        read_fallbacks: get(&c.read_fallbacks),
        read_loads: get(&c.read_loads),
        read_reloads: get(&c.read_reloads),
        read_errors: get(&c.read_errors),
        write_hits: get(&c.write_hits),
        write_misses: get(&c.write_misses),
        write_fallbacks: get(&c.write_fallbacks),
        write_aborts: get(&c.write_aborts),
        write_incremental: get(&c.write_incremental),
    }
}
