//! Task28 I1: the node-memory shadow follows committed batches on both engines,
//! evicts after writes outside batches, and does nothing when off.
//! Counters are process-wide, so each engine has one test and checks deltas.
mod postgres_support;

use fagent_storage::*;
use serde_json::json;
use std::sync::Arc;

fn create(id: &str, kind: NodeKind, parent: Option<&str>, props: serde_json::Value) -> Operation {
    Operation::CreateNode {
        id: id.into(),
        kind,
        parent_id: parent.map(str::to_owned),
        props,
        index: None,
    }
}

fn ops(value: serde_json::Value) -> Vec<Operation> {
    serde_json::from_value(value).unwrap()
}

/// Five batches (one scoped), an asset import and a privacy change. Returns the
/// number of committed batches.
fn exercise(storage: &Storage, slug: &str) -> u64 {
    let p = storage.create_project(slug, "Shadow").unwrap();
    let request = |revision: u64, operations: Vec<Operation>| BatchRequest {
        project_id: p.id().into(),
        expected_revision: Some(revision),
        strict: false,
        idempotency_key: format!("k{revision}"),
        operations,
    };
    let seed = vec![
        create("page", NodeKind::Page, None, json!({"name":"Home"})),
        create(
            "frame",
            NodeKind::Frame,
            Some("page"),
            json!({"width":400,"height":300}),
        ),
        create(
            "a",
            NodeKind::Rectangle,
            Some("frame"),
            json!({"width":10,"height":10}),
        ),
        create(
            "b",
            NodeKind::Rectangle,
            Some("frame"),
            json!({"width":20,"height":20}),
        ),
    ];
    let mut rev = p.apply_batch(request(0, seed)).unwrap().committed_revision;
    let update = ops(json!([{"op":"update_node","id":"a","props":{"fill":"#ff0000"}}]));
    rev = p
        .apply_batch(request(rev, update))
        .unwrap()
        .committed_revision;
    // A replay commits nothing and must not reach the shadow twice.
    let replay = ops(json!([{"op":"update_node","id":"a","props":{"fill":"#ff0000"}}]));
    assert!(p.apply_batch(request(rev - 1, replay)).unwrap().replayed);
    let asset = ImportAssetRequest {
        project_id: p.id().into(),
        expected_revision: rev,
        idempotency_key: "asset".into(),
        media_type: "image/png".into(),
        bytes: png(),
        font: None,
    };
    rev = p.import_asset(asset).unwrap().result.committed_revision;
    let reorder = ops(json!([{"op":"reorder_node","id":"b","index":0}]));
    rev = p
        .apply_batch(request(rev, reorder))
        .unwrap()
        .committed_revision;
    storage
        .update_project_privacy(p.id(), PrivacyStatus::Public)
        .unwrap();
    let scoped = ScopedBatchRequest {
        project_id: p.id().into(),
        expected_revision: Some(rev),
        strict: false,
        idempotency_key: "scoped".into(),
        page_ids: vec!["page".into()],
        operations: ops(json!([{"op":"delete_subtree","id":"a"}])),
    };
    p.apply_scoped_batch(scoped).unwrap();
    let notes = ops(json!([{"op":"create_note","id":"n1","nodeId":"b","text":"Hi"}]));
    p.apply_batch(request(rev + 1, notes)).unwrap();
    5
}

fn png() -> Vec<u8> {
    vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 11, 73, 68, 65, 84, 120, 156, 99, 0, 1, 0, 0, 5, 0, 1,
        13, 10, 45, 180, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ]
}

fn check(storage_for: impl Fn(ShadowMode) -> Storage) {
    let start = node_memory_shadow_stats();
    exercise(&storage_for(ShadowMode::Off), "off");
    assert_eq!(
        node_memory_shadow_stats(),
        start,
        "an off shadow does nothing"
    );
    let batches = exercise(&storage_for(ShadowMode::Strict), "strict");
    let end = node_memory_shadow_stats();
    assert_eq!(end.matched - start.matched, batches);
    assert_eq!(
        (end.mismatches, end.errors),
        (start.mismatches, start.errors)
    );
    // Seed loads; the import and the privacy change each evict and force a reload.
    assert_eq!(end.loads - start.loads, 3);
    assert_eq!(end.evictions - start.evictions, 2);
}

#[test]
fn sqlite_shadow_matches_and_evicts() {
    let temp = tempfile::tempdir().unwrap();
    check(|mode| {
        let root = temp.path().join(format!("{mode:?}"));
        Storage::new(SqliteBackend::new(
            Store::open_with_node_memory_writes(root, mode, ReadsMode::Off, WritesMode::Off)
                .unwrap(),
        ))
    });
}

#[test]
#[ignore = "requires fresh PostgreSQL via scripts/task19-check.sh"]
fn postgres_shadow_matches_and_evicts() {
    check(|mode| {
        let mut config = postgres_support::config();
        config.node_memory_shadow = mode;
        config.node_memory_reads = ReadsMode::Off;
        config.node_memory_writes = WritesMode::Off;
        Storage::from_shared(Arc::new(PostgresBackend::with_config(config).unwrap()))
    });
}
