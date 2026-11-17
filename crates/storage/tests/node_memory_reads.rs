//! Task28 I2: scoped snapshots come from memory only at the database's current
//! revision, on both engines; writes outside batches, other writers and unknown
//! targets fall back to today's snapshot; reads off never use memory.
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
    assert!(!read.verify);
    (read.from_memory, read.shared.snapshot.revision)
}

/// `other` writes to the same database without memory (PostgreSQL only).
fn check(on: &Storage, other: Option<Storage>, slug: &str) -> (String, u64) {
    for storage in [on].into_iter().chain(other.as_ref()) {
        let error = storage.scoped_snapshot("missing", "frame").err().unwrap();
        assert_eq!(error.code, "PROJECT_NOT_FOUND");
    }
    let id = on.create_project(slug, "Reads").unwrap().id().to_owned();
    let mut rev = commit(on, &id, 0, "seed", seed());
    // The mirror tagged the incarnation it loaded (Task28 I3): the first read hits.
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
    let on = Store::open_with_node_memory(temp.path(), ShadowMode::Strict, ReadsMode::On);
    let on: Storage = on.unwrap().into();
    let start = node_memory_shadow_stats();
    check(&on, None, "sqlite-on");
    let end = node_memory_shadow_stats();
    assert!(end.read_hits - start.read_hits >= 4);
    let off = Store::open_with_node_memory_writes(
        temp.path().join("off"),
        ShadowMode::Off,
        ReadsMode::Off,
        WritesMode::Off,
    );
    let off: Storage = off.unwrap().into();
    let p = off.create_project("off", "Off").unwrap();
    commit(&off, p.id(), 0, "seed", seed());
    assert_eq!(read(&off, p.id(), "frame"), (false, 1));
}

#[test]
#[ignore = "requires fresh PostgreSQL via scripts/task19-check.sh"]
fn postgres_scoped_reads_follow_other_writers() {
    let storage = |reads: ReadsMode| {
        let mut config = postgres_support::config();
        config.node_memory_shadow = ShadowMode::Strict;
        config.node_memory_reads = reads;
        config.node_memory_writes = match reads {
            ReadsMode::Off => WritesMode::Off,
            _ => WritesMode::from_env(),
        };
        Storage::from_shared(Arc::new(PostgresBackend::with_config(config).unwrap()))
    };
    let on = storage(ReadsMode::On);
    let (id, rev) = check(&on, Some(storage(ReadsMode::Off)), "postgres-reads");
    assert_eq!(read(&storage(ReadsMode::Off), &id, "frame"), (false, rev));
}
