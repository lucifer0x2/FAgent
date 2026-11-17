//! Freshness and fallback rules of memory reads, with scripted probes and loads.
use super::mirror::Mirror;
use super::read_mode::{Loaded, ReadsMode, ScopedRead};
use super::reads::Reads;
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
    mirror: Mirror,
    reads: Reads,
    loads: Cell<usize>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            mirror: Mirror::new(false),
            reads: Reads::new(ReadsMode::On).unwrap(),
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
        self.reads
            .read(&self.mirror, target, || probe, load)
            .unwrap()
    }

    fn hot(&self) -> Option<u64> {
        let handle = self.mirror.registry.get("proj")?;
        let revision = handle.read().revision();
        Some(revision)
    }
}

#[test]
fn cold_loads_then_serves_a_view_at_the_probed_revision() {
    let f = Fixture::new();
    assert!(!f.read("frame", 3, "i").from_memory);
    assert_eq!(f.hot(), Some(3));
    let hit = f.read("frame", 3, "i");
    assert!(hit.from_memory && !hit.verify);
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
}

/// An observer the mirror skipped or one not yet run leaves memory behind the
/// database: the read never serves it, falls back and reloads.
#[test]
fn behind_or_another_incarnation_reloads_and_ahead_falls_back() {
    let f = Fixture::new();
    f.read("frame", 3, "i");
    assert!(!f.read("frame", 4, "i").from_memory);
    assert_eq!(f.hot(), Some(4));
    assert!(f.read("frame", 4, "i").from_memory);
    assert!(!f.read("frame", 4, "restored").from_memory);
    assert!(f.read("frame", 4, "restored").from_memory);
    // Memory ahead of a probe (a commit landed after it): today's path, no reload.
    assert!(!f.read("frame", 3, "restored").from_memory);
    assert_eq!(f.hot(), Some(4));
}

#[test]
fn untagged_projects_from_the_mirror_reload_once() {
    let f = Fixture::new();
    f.mirror.registry.load(snapshot(5, None)).unwrap();
    assert!(!f.read("frame", 5, "i").from_memory);
    assert!(f.read("frame", 5, "i").from_memory);
}

#[test]
fn probe_failures_fall_back_and_load_errors_are_todays() {
    let f = Fixture::new();
    let error = || StorageError::new("PROJECT_NOT_FOUND", "Project does not exist");
    let read = f.read_with("frame", Err(error()), || Ok(loaded(snapshot(1, None), "i")));
    assert!(!read.from_memory);
    assert_eq!(f.hot(), None, "no identity, no load");
    let failed = f
        .reads
        .read(&f.mirror, "frame", || Err(error()), || Err(error()));
    assert_eq!(failed.err().unwrap().code, "PROJECT_NOT_FOUND");
}

#[test]
fn snapshots_the_lib_rejects_are_not_retried() {
    let f = Fixture::new();
    let cyclic = || Ok(loaded(snapshot(2, Some("a")), "i"));
    let probe = || Ok(Some(identity(2, "i")));
    assert!(!f.read_with("frame", probe(), cyclic).from_memory);
    assert!(f.reads.failed("proj"));
    assert!(!f.read_with("frame", probe(), cyclic).from_memory);
    assert_eq!(f.hot(), None);
    // A new revision is tried again.
    assert!(!f.read("frame", 3, "i").from_memory);
    assert!(!f.reads.failed("proj"));
    assert!(f.read("frame", 3, "i").from_memory);
}

#[test]
fn i5_hot_ownership_suppresses_late_cache_fills_and_releases_on_eviction() {
    use super::{Shadow, ShadowMode, WritesMode};
    use crate::{revision_indexes::IndexCache, SnapshotCache};
    let shadow = Shadow::new(ShadowMode::Off, ReadsMode::On, WritesMode::Off);
    let cache = SnapshotCache::new("test").with_owner(shadow.clone());
    let indexes = IndexCache::default().with_owner(shadow.clone());
    let snapshot = Arc::new(snapshot(1, None));
    cache.put(&snapshot, 1);
    let old = indexes.for_snapshot(&snapshot, Some(identity(1, "i")));
    assert!(cache.get("proj", 1, 1).is_some());
    let read = shadow
        .scoped_read(
            "",
            || Ok(Some(identity(1, "i"))),
            || Ok(loaded(snapshot.as_ref().clone(), "i")),
        )
        .unwrap();
    assert!(!read.from_memory);
    assert!(shadow.claim_cache_cleanup("proj"));
    assert!(!shadow.claim_cache_cleanup("proj"));
    shadow.retag(&identity(1, "i"));
    assert!(
        !shadow.claim_cache_cleanup("proj"),
        "retag keeps transition state"
    );
    indexes.evict("proj");
    cache.evict("proj");
    cache.put(&snapshot, 1); // a read/commit begun before admission finishes late
    indexes.publish(identity(1, "i"), old.clone());
    assert!(cache.get("proj", 1, 1).is_none());
    assert!(!Arc::ptr_eq(
        &old,
        &indexes.for_snapshot(&snapshot, Some(identity(1, "i")))
    ));
    let retained = shadow
        .scoped_read(
            "",
            || Ok(Some(identity(1, "i"))),
            || panic!("HOT must not load"),
        )
        .unwrap();
    assert!(retained.from_memory);
    shadow.evict("proj");
    assert!(retained.shared.scoped().unwrap().node("a").is_some());
    cache.put(&snapshot, 1);
    assert!(cache.get("proj", 1, 1).is_some());
    shadow
        .scoped_read(
            "",
            || Ok(Some(identity(1, "i"))),
            || Ok(loaded(snapshot.as_ref().clone(), "i")),
        )
        .unwrap();
    assert!(
        shadow.claim_cache_cleanup("proj"),
        "reload gets a new transition"
    );
    let mirror_only = Shadow::new(ShadowMode::On, ReadsMode::Off, WritesMode::Off);
    mirror_only
        .mirror
        .as_ref()
        .unwrap()
        .registry
        .load(snapshot.as_ref().clone())
        .unwrap();
    assert!(!mirror_only.owns_project("proj"));
}
