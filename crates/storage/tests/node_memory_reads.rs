//! Scoped snapshots come from node memory only at the database's current revision,
//! on both engines. Commits stage in memory and leave the project resident at the
//! new revision; writes outside batches evict it, so the next read loads it; other
//! writers and unknown targets fall back to the database snapshot.
mod postgres_support;

use fagent_storage::*;
use serde_json::{json, Value};
use std::sync::Arc;

fn commit(storage: &Storage, project: &str, revision: u64, key: &str, ops: Value) -> u64 {
    let request = BatchRequest {
        project_id: project.into(),
        expected_revision: Some(revision),
        strict: false,
        idempotency_key: key.into(),
        operations: serde_json::from_value(ops).unwrap(),
    };
    let p = storage.open_project(project).unwrap();
    p.apply_batch(request).unwrap().committed_revision
}

fn seed() -> Value {
    let node = |id: &str, kind: &str, parent: Option<&str>| json!({"op":"create_node","id":id,"kind":kind,"parentId":parent,"props":{"width":10,"height":10}});
    json!([
        {"op":"create_node","id":"page","kind":"page","parentId":null,"props":{"name":"Home"}},
        node("frame", "frame", Some("page")),
        node("a", "rectangle", Some("frame")),
        node("other", "frame", Some("page")),
    ])
}

/// `(from_memory, revision)` of a scoped read of `target`.
fn read(storage: &Storage, project: &str, target: &str) -> (bool, u64) {
    let read = storage.scoped_snapshot(project, target).unwrap();
    (read.from_memory, read.shared.snapshot.revision)
}

/// `other` writes to the same database through its own memory (PostgreSQL only).
fn check(on: &Storage, other: Option<Storage>, slug: &str) -> (String, u64) {
    for storage in [on].into_iter().chain(other.as_ref()) {
        let error = storage.scoped_snapshot("missing", "frame").err().unwrap();
        assert_eq!(error.code, "PROJECT_NOT_FOUND");
    }
    let id = on.create_project(slug, "Reads").unwrap().id().to_owned();
    let mut rev = commit(on, &id, 0, "seed", seed());
    // The commit left the project resident at its revision.
    assert_eq!(read(on, &id, "frame"), (true, rev));
    assert_eq!(read(on, slug, "a"), (true, rev), "slugs resolve too");
    assert!(!read(on, &id, "missing").0);
    rev = commit(
        on,
        &id,
        rev,
        "edit",
        json!([{"op":"update_node","id":"a","props":{"name":"A"}}]),
    );
    assert_eq!(read(on, &id, "frame"), (true, rev), "committed in memory");
    let hit = on.scoped_snapshot(&id, "frame").unwrap();
    assert!(hit.from_memory);
    let scoped = hit.shared.scoped().unwrap();
    let a = scoped.node("a").unwrap();
    assert_eq!(
        (hit.shared.snapshot.revision, &a.props["name"]),
        (rev, &json!("A"))
    );
    on.update_project_privacy(&id, PrivacyStatus::Public)
        .unwrap();
    assert_eq!(read(on, &id, "frame"), (false, rev), "evicted: cold load");
    assert_eq!(read(on, &id, "frame"), (true, rev));
    if let Some(other) = other {
        rev = commit(
            &other,
            &id,
            rev,
            "elsewhere",
            json!([{"op":"delete_subtree","id":"a"}]),
        );
        assert_eq!(read(on, &id, "frame"), (false, rev), "behind: reload");
        let hit = on.scoped_snapshot(&id, "frame").unwrap();
        assert!(hit.from_memory && hit.shared.scoped().unwrap().node("a").is_none());
    }
    (id, rev)
}

#[test]
fn sqlite_scoped_reads_come_from_memory_when_current() {
    let temp = tempfile::tempdir().unwrap();
    let on: Storage = Store::open(temp.path()).unwrap().into();
    check(&on, None, "sqlite-on");
    let stats = on.memory_stats();
    assert!(stats.read_hits >= 4, "{stats:?}");
    assert!(stats.read_loads >= 1, "{stats:?}");
    assert!(stats.write_loads >= 1 && stats.write_hits >= 1, "{stats:?}");
    // Every load is admitted (fully validated once); the counters are serialized.
    assert_eq!(stats.memory.admissions, stats.memory.loads, "{stats:?}");
    assert_eq!(stats.memory.admission_failures, 0, "{stats:?}");
    let json = serde_json::to_value(stats).unwrap();
    for key in [
        "admissions",
        "admission_failures",
        "admission_micros",
        "rejected_direct",
    ] {
        assert!(json["memory"].get(key).is_some(), "{key} in {json}");
    }
}

#[test]
#[ignore = "requires fresh PostgreSQL via scripts/task19-check.sh"]
fn postgres_scoped_reads_follow_other_writers() {
    let storage = || {
        let config = postgres_support::config();
        Storage::from_shared(Arc::new(PostgresBackend::with_config(config).unwrap()))
    };
    let on = storage();
    let (id, rev) = check(&on, Some(storage()), "postgres-reads");
    let fresh = storage();
    assert_eq!(
        read(&fresh, &id, "frame"),
        (false, rev),
        "a new process starts cold"
    );
    assert_eq!(read(&fresh, &id, "frame"), (true, rev));
}
