//! Shadow modes, strict mirroring of SQLite commits and evictions.
use super::*;
use crate::{BatchRequest, PrivacyStatus, Project, Store};
use serde_json::{json, Value};

fn store(mode: ShadowMode) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_with_shadow(dir.path(), mode).unwrap();
    (dir, store)
}

fn apply(p: &Project, revision: u64, ops: Value) -> u64 {
    let request = BatchRequest {
        project_id: p.id().into(),
        expected_revision: Some(revision),
        strict: false,
        idempotency_key: format!("k{revision}"),
        operations: serde_json::from_value(ops).unwrap(),
    };
    p.apply_batch(request).unwrap().committed_revision
}

fn hot(store: &Store, p: &Project) -> Option<u64> {
    let mirror = store.root.shadow.mirror.as_ref().unwrap();
    mirror.hot_revision(p.id())
}

fn node(id: &str, kind: &str, parent: Option<&str>, props: Value) -> Value {
    json!({"op":"create_node","id":id,"kind":kind,"parentId":parent,"props":props})
}

fn seed() -> Value {
    json!([
        node("page", "page", None, json!({"name":"Home"})),
        node(
            "frame",
            "frame",
            Some("page"),
            json!({"width":800,"height":600,"layout":{"mode":"column","gap":8}})
        ),
        node(
            "a",
            "rectangle",
            Some("frame"),
            json!({"width":10,"height":10,"fill":"#ff0000"})
        ),
        node(
            "b",
            "rectangle",
            Some("frame"),
            json!({"width":20,"height":20})
        ),
        node(
            "c",
            "frame",
            Some("frame"),
            json!({"width":100,"height":100})
        ),
        node("d", "rectangle", Some("c"), json!({"width":5,"height":5})),
    ])
}

#[test]
fn modes_parse_from_text() {
    assert_eq!(ShadowMode::parse(None), ShadowMode::Off);
    assert_eq!(ShadowMode::parse(Some("0")), ShadowMode::Off);
    assert_eq!(ShadowMode::parse(Some("1")), ShadowMode::On);
    assert_eq!(ShadowMode::parse(Some(" on ")), ShadowMode::On);
    assert_eq!(ShadowMode::parse(Some("strict")), ShadowMode::Strict);
    assert!(!Shadow::new(ShadowMode::Off, ReadsMode::Off, Default::default()).enabled());
    assert!(Shadow::new(ShadowMode::Off, ReadsMode::On, Default::default()).enabled());
    assert_eq!(ReadsMode::parse(None), ReadsMode::Off);
    assert_eq!(ReadsMode::parse(Some("on")), ReadsMode::On);
    assert_eq!(ReadsMode::parse(Some("strict")), ReadsMode::Strict);
}

/// Strict mode panics on any difference, including the full row set.
#[test]
fn strict_shadow_follows_every_kind_of_sqlite_commit() {
    let (_dir, store) = store(ShadowMode::Strict);
    let p = store.create_project("shadow", "Shadow").unwrap();
    let mut rev = apply(&p, 0, seed());
    assert_eq!(hot(&store, &p), Some(rev));
    let batches = [
        json!([{"op":"update_node","id":"a","props":{"fill":"#00ff00","opacity":0.5}}]),
        json!([{"op":"update_node","id":"frame","props":{"layout":{"gap":12}},"merge":true}]),
        json!([{"op":"update_node","id":"b","props":{"width":20}}]),
        json!([{"op":"reorder_node","id":"b","index":0}]),
        json!([{"op":"reparent_node","id":"a","parentId":"c","index":0}]),
        json!([{"op":"create_note","id":"n1","nodeId":"d","text":"Check","author":"qa"},
               {"op":"reply_note","id":"n1","text":"Done"}]),
        json!([{"op":"upsert_token","token":{"id":"tok_brand","name":"color.brand","type":"color","value":"#123456"}}]),
        json!([{"op":"delete_subtree","id":"c"}]),
        json!([node("e", "rectangle", Some("frame"), json!({"width":1,"height":1})),
               {"op":"reorder_node","id":"e","index":0}]),
    ];
    for ops in batches {
        rev = apply(&p, rev, ops);
        assert_eq!(hot(&store, &p), Some(rev));
    }
}

#[test]
fn writes_outside_batches_evict_and_the_next_commit_reloads() {
    let (_dir, store) = store(ShadowMode::Strict);
    let p = store.create_project("evict", "Evict").unwrap();
    let rev = apply(&p, 0, seed());
    assert_eq!(hot(&store, &p), Some(rev));
    p.update_privacy(PrivacyStatus::Public).unwrap();
    assert_eq!(hot(&store, &p), None);
    let rev = apply(
        &p,
        rev,
        json!([{"op":"update_node","id":"a","props":{"width":11}}]),
    );
    assert_eq!(hot(&store, &p), Some(rev));
}

#[test]
fn a_diverging_row_is_reported_and_evicted() {
    let (_dir, store) = store(ShadowMode::On);
    let p = store.create_project("diverge", "Diverge").unwrap();
    let rev = apply(&p, 0, seed());
    let before = p.snapshot().unwrap();
    let mirror = Mirror::new(false);
    let result = |revision: u64, ids: &[&str]| crate::BatchResult {
        project_id: p.id().into(),
        committed_revision: revision,
        changed_ids: ids.iter().map(|s| s.to_string()).collect(),
        render_status: "pending".into(),
        ..Default::default()
    };
    let ops: Vec<Operation> =
        serde_json::from_value(json!([{"op":"update_node","id":"a","props":{"width":12}}]))
            .unwrap();
    let mut wrong = Rows::default();
    let mut row = before.nodes.iter().find(|n| n.id == "a").unwrap().clone();
    row.props["width"] = json!(13);
    wrong.upsert(Kind::Node, "a", &row);
    let commit = Commit {
        before: &before,
        operations: &ops,
        result: &result(rev + 1, &["a"]),
        rows: &wrong,
        identity: None,
        ready: None,
    };
    match mirror.run(&commit) {
        Err(Failure::Mismatch(m)) => {
            assert_eq!((m.reason.as_str(), m.ids), ("row.node", vec!["a".into()]))
        }
        _ => panic!("expected a row mismatch"),
    }
    // The staged batch was rolled back: memory stays at the base revision.
    assert_eq!(mirror.hot_revision(p.id()), Some(rev));
    let ids = Commit {
        result: &result(rev + 1, &["a", "b"]),
        ..commit
    };
    assert!(matches!(mirror.run(&ids), Err(Failure::Mismatch(m)) if m.reason == "changed_ids"));
    mirror.fail(&ids, Failure::Error("test".into()));
    assert_eq!(mirror.hot_revision(p.id()), None);
}
