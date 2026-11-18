//! `NodeMemory`: loading, lookup, TTL and budget with a manual clock, expiry rate
//! limiting, counters and snapshots.
mod support;

use fagent_node_memory::{Eviction, Limits, ManualClock, MemoryConfig, NodeMemory};
use fagent_scene_input::Snapshot;
use std::sync::Arc;
use std::time::Duration;
use support::*;

fn project(id: &str) -> Snapshot {
    let mut snapshot = small();
    snapshot.project_id = id.into();
    snapshot
}

fn memory(ttl_secs: u64, budget: usize) -> (NodeMemory, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::default());
    let config = MemoryConfig {
        ttl: Duration::from_secs(ttl_secs),
        budget_bytes: budget,
        expire_every: Duration::from_secs(60),
        limits: Limits::default(),
    };
    (NodeMemory::new(config, clock.clone()), clock)
}

#[test]
fn defaults_are_a_one_hour_ttl_and_a_two_gib_budget() {
    let config = MemoryConfig::default();
    assert_eq!(config.ttl, Duration::from_secs(3600));
    assert_eq!(config.budget_bytes, 2 << 30);
    assert_eq!(config.expire_every, Duration::from_secs(60));
    assert_eq!(config.limits, Limits::default());
}

#[test]
fn load_project_resident_and_evict() {
    let (memory, clock) = memory(60, usize::MAX);
    assert!(memory.project("a").is_none());
    assert!(memory.resident("a").is_none());
    let a = memory.load(project("a")).unwrap();
    assert_eq!(a.id(), "a");
    assert_eq!(a.view().metadata().revision, 1);
    assert_eq!(a.view().len(), 18);
    assert_eq!(memory.resident("a"), Some(a.load_id()));
    assert_eq!(memory.project("a").unwrap().load_id(), a.load_id());
    let b = memory.load(project("b")).unwrap();
    assert!(b.load_id() > a.load_id());
    assert_eq!(
        memory.total_bytes(),
        a.estimated_bytes() + b.estimated_bytes()
    );
    clock.advance(Duration::from_secs(1));
    assert!(memory.evict("a"));
    assert!(!memory.evict("a"));
    assert!(memory.resident("a").is_none());
    // The holder keeps its handle; a reload is a new load.
    assert_eq!(a.view().len(), 18);
    let again = memory.load(project("a")).unwrap();
    assert!(again.load_id() > b.load_id());
    assert_ne!(memory.resident("a"), Some(a.load_id()));
}

#[test]
fn second_load_is_project_exists_and_not_a_failure() {
    let (memory, _) = memory(60, usize::MAX);
    memory.load(project("a")).unwrap();
    let error = memory.load(project("a")).expect_err("already resident");
    assert_eq!(error.code, "PROJECT_EXISTS");
    let stats = memory.stats();
    assert_eq!((stats.loads, stats.load_failures), (1, 0));
}

#[test]
fn invalid_snapshot_counts_a_load_failure() {
    let (memory, _) = memory(60, usize::MAX);
    let mut bad = project("a");
    bad.nodes[3].parent_id = Some("missing".into());
    assert!(memory.load(bad).is_err());
    assert!(memory.resident("a").is_none());
    let stats = memory.stats();
    assert_eq!((stats.loads, stats.load_failures), (0, 1));
}

#[test]
fn project_refreshes_access_time_but_resident_does_not() {
    let (memory, clock) = memory(10, usize::MAX);
    memory.load(project("touched")).unwrap();
    memory.load(project("peeked")).unwrap();
    clock.advance(Duration::from_secs(6));
    memory.project("touched").unwrap();
    memory.resident("peeked").unwrap();
    clock.advance(Duration::from_secs(6));
    let evicted = memory.expire();
    assert_eq!(evicted, [Eviction::Expired("peeked".into())]);
    assert!(memory.resident("touched").is_some());
    assert_eq!(memory.stats().expired, 1);
}

#[test]
fn ttl_expiry_keeps_the_project_a_load_just_added() {
    let (memory, clock) = memory(10, usize::MAX);
    memory.load(project("old")).unwrap();
    clock.advance(Duration::from_secs(10));
    memory.load(project("new")).unwrap();
    assert!(memory.resident("old").is_none());
    assert!(memory.resident("new").is_some());
    assert_eq!(memory.stats().expired, 1);
}

#[test]
fn budget_evicts_least_recently_used_first() {
    let one = memory(60, usize::MAX).0.load(project("x")).unwrap();
    let (memory, clock) = memory(60, one.estimated_bytes() * 2 + 1);
    memory.load(project("a")).unwrap();
    clock.advance(Duration::from_secs(1));
    memory.load(project("b")).unwrap();
    clock.advance(Duration::from_secs(1));
    memory.project("a").unwrap();
    clock.advance(Duration::from_secs(1));
    // The third load pushes the total over the budget: `b` is the least recent.
    memory.load(project("c")).unwrap();
    assert!(memory.resident("b").is_none());
    assert!(memory.resident("a").is_some() && memory.resident("c").is_some());
    let stats = memory.stats();
    assert_eq!((stats.over_budget, stats.expired), (1, 0));
}

#[test]
fn maybe_expire_is_rate_limited_by_the_clock() {
    let (memory, clock) = memory(10, usize::MAX);
    memory.load(project("a")).unwrap();
    clock.advance(Duration::from_secs(30));
    // Idle past the TTL, but the last sweep is too recent.
    assert!(memory.maybe_expire().is_empty());
    assert!(memory.resident("a").is_some());
    clock.advance(Duration::from_secs(30));
    assert_eq!(memory.maybe_expire(), [Eviction::Expired("a".into())]);
    // The sweep reset the rate limit.
    memory.load(project("b")).unwrap();
    clock.advance(Duration::from_secs(59));
    assert!(memory.maybe_expire().is_empty());
    assert!(memory.resident("b").is_some());
    clock.advance(Duration::from_secs(1));
    assert_eq!(memory.maybe_expire(), [Eviction::Expired("b".into())]);
}

#[test]
fn explicit_expire_resets_the_rate_limit() {
    let (memory, clock) = memory(10, usize::MAX);
    memory.load(project("a")).unwrap();
    clock.advance(Duration::from_secs(59));
    memory.expire();
    clock.advance(Duration::from_secs(2));
    assert!(
        memory.maybe_expire().is_empty(),
        "61 s since start, 2 s since sweep"
    );
}

/// Every reading advances time by 7 microseconds.
struct Tick(std::sync::atomic::AtomicU64);

impl fagent_node_memory::Clock for Tick {
    fn now(&self) -> Duration {
        let micros = self.0.fetch_add(7, std::sync::atomic::Ordering::SeqCst);
        Duration::from_micros(micros)
    }
}

#[test]
fn load_micros_come_from_the_clock_only() {
    let (manual, _) = memory(60, usize::MAX);
    manual.load(project("a")).unwrap();
    assert_eq!(manual.stats().load_micros, 0);
    let ticking = NodeMemory::new(MemoryConfig::default(), Arc::new(Tick(Default::default())));
    ticking.load(project("a")).unwrap();
    let stats = ticking.stats();
    assert!(
        stats.load_micros > 0 && stats.load_micros.is_multiple_of(7),
        "{stats:?}"
    );
}

#[test]
fn to_snapshot_is_the_public_order_and_matches_the_load() {
    let (memory, _) = memory(60, usize::MAX);
    assert!(memory.to_snapshot("a").is_none());
    let mut input = project("a");
    input.tokens = serde_json::from_value(serde_json::json!([
        {"id":"b","name":"B","type":"color","value":"#000000"},
        {"id":"a","name":"A","type":"color","value":"#ffffff"}
    ]))
    .unwrap();
    memory.load(input.clone()).unwrap();
    let out = memory.to_snapshot("a").unwrap();
    assert_eq!(out.revision, 1);
    assert_eq!(
        out.tokens.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        ["a", "b"]
    );
    let mut expected = input.nodes;
    expected
        .sort_by(|a, b| (&a.parent_id, a.sort_key, &a.id).cmp(&(&b.parent_id, b.sort_key, &b.id)));
    assert!(out.nodes == expected, "nodes in public order");
    assert_eq!(out.fonts, input.fonts);
}

#[test]
fn settled_view_returns_the_published_revision() {
    let (memory, _) = memory(60, usize::MAX);
    let a = memory.load(project("a")).unwrap();
    assert_eq!(a.settled_view(1).metadata().revision, 1);
    // Nothing is staged, so waiting for a newer revision returns immediately.
    assert_eq!(a.settled_view(5).metadata().revision, 1);
}

#[test]
fn custom_limits_apply_to_loads() {
    let clock = Arc::new(ManualClock::default());
    let config = MemoryConfig {
        limits: Limits { max_nodes: 3 },
        ..MemoryConfig::default()
    };
    let memory = NodeMemory::new(config, clock);
    let error = memory.load(project("a")).expect_err("over the cap");
    assert_eq!(error.field.as_deref(), Some("nodes"));
}
