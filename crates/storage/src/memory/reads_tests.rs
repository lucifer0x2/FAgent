//! Freshness and fallback rules of memory reads, with scripted probes and loads.
use super::{Loaded, Memory, ScopedRead};
use crate::{RevisionIndexes, SharedSnapshot, Snapshot, SnapshotCacheIdentity, StorageError};
use serde_json::json;
use std::cell::Cell;
use std::sync::Arc;

fn snapshot(revision: u64, parent_of_page: Option<&str>) -> Snapshot {
    let node = |id: &str, kind: &str, parent: Option<&str>| json!({"id":id,"kind":kind,"parentId":parent,"sortKey":0,"props":{}});
    serde_json::from_value(json!({
        "schemaVersion":4,"projectId":"proj","slug":"proj","name":"Proj","revision":revision,
        "nodes":[node("page","page",parent_of_page), node("frame","frame",Some("page")),
            node("a","rectangle",Some("frame"))],
        "tokens":[],"styles":[],"assets":[],"fonts":[],"notes":[],
        "retention":{"oldestCommitRevision":0,"idempotencyPolicy":"bounded"}
    }))
    .unwrap()
}

fn identity(revision: u64, incarnation: &str) -> SnapshotCacheIdentity {
    SnapshotCacheIdentity {
        project_id: "proj".into(),
        schema_version: 4,
        revision,
        incarnation: incarnation.into(),
        asset_readiness: vec![],
    }
}

fn loaded(snapshot: Snapshot, incarnation: &str) -> Loaded {
    let identity = identity(snapshot.revision, incarnation);
    let indexes = Arc::new(RevisionIndexes::new(&snapshot.nodes));
    let shared = SharedSnapshot::committed(Arc::new(snapshot), indexes);
    (shared, Some(identity))
}

struct Fixture {
    memory: Memory,
    loads: Cell<usize>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            memory: Memory::default(),
            loads: Cell::new(0),
        }
    }

    /// One read with the database at `revision` of `incarnation`.
    fn read(&self, target: &str, revision: u64, incarnation: &str) -> ScopedRead {
        self.read_with(target, Ok(Some(identity(revision, incarnation))), || {
            Ok(loaded(snapshot(revision, None), incarnation))
        })
    }

    fn read_with(
        &self,
        target: &str,
        probe: crate::Result<Option<SnapshotCacheIdentity>>,
        load: impl FnOnce() -> crate::Result<Loaded>,
    ) -> ScopedRead {
        let load = || {
            self.loads.set(self.loads.get() + 1);
            load()
        };
        self.memory.scoped_read(target, || probe, load).unwrap()
    }

    fn hot(&self) -> Option<u64> {
        let project = self.memory.engine().project("proj")?;
        let revision = project.view().metadata().revision;
        Some(revision)
    }
}

#[test]
fn cold_loads_then_serves_a_view_at_the_probed_revision() {
    let f = Fixture::new();
    assert!(!f.read("frame", 3, "i").from_memory);
    assert_eq!(f.hot(), Some(3));
    let hit = f.read("frame", 3, "i");
    assert!(hit.from_memory);
    assert_eq!(hit.shared.snapshot.revision, 3);
    assert!(hit.shared.trusted_scoped().unwrap().is_some());
    assert!(
        f.read("frame/a", 3, "i").from_memory,
        "virtual ids read through the authored instance"
    );
    assert!(!f.read("missing", 3, "i").from_memory);
    assert_eq!(
        f.loads.get(),
        2,
        "only the cold read and the unknown target load"
    );
    let stats = f.memory.stats();
    assert_eq!((stats.read_hits, stats.read_fallbacks), (2, 2));
    assert_eq!((stats.read_loads, stats.memory.loads), (1, 1));
}

/// A commit evicts memory, but a read may have loaded an older revision meanwhile:
/// the read never serves it, falls back and reloads.
#[test]
fn behind_or_another_incarnation_reloads_and_ahead_falls_back() {
    let f = Fixture::new();
    f.read("frame", 3, "i");
    assert!(!f.read("frame", 4, "i").from_memory);
    assert_eq!(f.hot(), Some(4));
    assert!(f.read("frame", 4, "i").from_memory);
    assert!(!f.read("frame", 4, "restored").from_memory);
    assert!(f.read("frame", 4, "restored").from_memory);
    // Memory ahead of a probe (a commit landed after it): the database, no reload.
    assert!(!f.read("frame", 3, "restored").from_memory);
    assert_eq!(f.hot(), Some(4));
    assert_eq!(f.memory.stats().read_reloads, 2);
}

#[test]
fn untagged_loads_reload_once() {
    let f = Fixture::new();
    f.memory.engine().load(snapshot(5, None)).unwrap();
    assert!(!f.read("frame", 5, "i").from_memory);
    assert!(f.read("frame", 5, "i").from_memory);
}

#[test]
fn eviction_makes_the_next_read_cold() {
    let f = Fixture::new();
    f.read("frame", 3, "i");
    assert!(f.read("frame", 3, "i").from_memory);
    f.memory.evict("proj");
    assert_eq!(f.hot(), None);
    assert!(!f.read("frame", 3, "i").from_memory);
    assert!(f.read("frame", 3, "i").from_memory);
    assert_eq!(f.memory.stats().read_loads, 2);
}

#[test]
fn probe_failures_fall_back_and_load_errors_are_the_databases() {
    let f = Fixture::new();
    let error = || StorageError::new("PROJECT_NOT_FOUND", "Project does not exist");
    let read = f.read_with("frame", Err(error()), || Ok(loaded(snapshot(1, None), "i")));
    assert!(!read.from_memory);
    assert_eq!(f.hot(), None, "no identity, no load");
    let failed = f
        .memory
        .scoped_read("frame", || Err(error()), || Err(error()));
    assert_eq!(failed.err().unwrap().code, "PROJECT_NOT_FOUND");
}

#[test]
fn snapshots_the_engine_rejects_are_not_retried() {
    let f = Fixture::new();
    let cyclic = || Ok(loaded(snapshot(2, Some("a")), "i"));
    let probe = || Ok(Some(identity(2, "i")));
    assert!(!f.read_with("frame", probe(), cyclic).from_memory);
    assert!(f.memory.failed("proj"));
    assert!(!f.read_with("frame", probe(), cyclic).from_memory);
    assert_eq!(f.hot(), None);
    assert_eq!(f.memory.stats().memory.load_failures, 1);
    // A new revision is tried again.
    assert!(!f.read("frame", 3, "i").from_memory);
    assert!(!f.memory.failed("proj"));
    assert!(f.read("frame", 3, "i").from_memory);
}

#[test]
fn ownership_suppresses_late_cache_fills_and_releases_on_eviction() {
    use crate::SnapshotCache;
    let memory = Memory::default();
    let cache = SnapshotCache::new("test").with_owner(memory.clone());
    let snapshot = Arc::new(snapshot(1, None));
    cache.put(&snapshot, 1);
    assert!(cache.get("proj", 1, 1).is_some());
    let read = memory
        .scoped_read(
            "",
            || Ok(Some(identity(1, "i"))),
            || Ok(loaded(snapshot.as_ref().clone(), "i")),
        )
        .unwrap();
    assert!(!read.from_memory);
    assert!(memory.owns_project("proj"));
    assert!(memory.claim_cache_cleanup("proj"));
    assert!(!memory.claim_cache_cleanup("proj"));
    cache.evict("proj");
    cache.put(&snapshot, 1); // a read/commit begun before admission finishes late
    assert!(cache.get("proj", 1, 1).is_none());
    let retained = memory
        .scoped_read(
            "",
            || Ok(Some(identity(1, "i"))),
            || panic!("a resident project must not load"),
        )
        .unwrap();
    assert!(retained.from_memory);
    memory.evict("proj");
    assert!(!memory.owns_project("proj"));
    assert!(retained.shared.scoped().unwrap().node("a").is_some());
    cache.put(&snapshot, 1);
    assert!(cache.get("proj", 1, 1).is_some());
    memory
        .scoped_read(
            "",
            || Ok(Some(identity(1, "i"))),
            || Ok(loaded(snapshot.as_ref().clone(), "i")),
        )
        .unwrap();
    assert!(
        memory.claim_cache_cleanup("proj"),
        "reload gets a new transition"
    );
    let untagged = Memory::default();
    untagged.engine().load(snapshot.as_ref().clone()).unwrap();
    assert!(!untagged.owns_project("proj"));
}
