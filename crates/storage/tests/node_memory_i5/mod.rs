//! Bounded end-to-end HOT dispatch, strict-page conflict and SQL rollback oracle.
use super::*;

#[test]
fn i5_sqlite_stale_strict_dispatch_rollback_and_retained_reader() {
    let temp = tempfile::tempdir().unwrap();
    let pair = sqlite_pair(temp.path());
    for name in ["off", "on"] {
        let db = rusqlite::Connection::open(temp.path().join(name).join("diff/db.sql")).unwrap();
        db.execute_batch("CREATE TRIGGER i5_fail BEFORE UPDATE ON nodes WHEN json_extract(NEW.props_json,'$.name')='fail' BEGIN SELECT RAISE(ABORT,'i5 forced failure'); END;").unwrap();
    }
    pair.seed();
    let send = |key: &str, base: u64, operations: Value, pages: bool| {
        let mut request = json!({"projectId":pair.project,"expectedRevision":base,"idempotencyKey":key,"operations":operations});
        if pages {
            request["strict"] = json!(true);
            request["pageIds"] = json!(["pg"]);
        }
        pair.both(&request, false)
    };
    let unrelated = send(
        "other",
        1,
        json!([{"op":"upsert_token","token":{"id":"t0","name":"Changed unused","type":"color","value":"#445566"}}]),
        false,
    );
    assert_eq!(unrelated["committedRevision"], 2);
    let retained = pair.on.reader_snapshot(&pair.project).unwrap();
    assert!(retained.from_memory);
    let start = node_memory_shadow_stats();
    let edit = json!([{"op":"update_node","id":"r0","props":{"name":"Allowed stale"}}]);
    let result = send("stale", 1, edit.clone(), true);
    assert_eq!(result["committedRevision"], 3);
    assert!(node_memory_shadow_stats().write_incremental > start.write_incremental);
    assert_eq!(send("stale", 1, edit, true)["replayed"], true);
    assert_eq!(retained.shared.snapshot.revision, 2);
    assert!(retained
        .shared
        .scoped()
        .unwrap()
        .node("r0")
        .unwrap()
        .props
        .get("name")
        .is_none());
    let master = send(
        "master",
        3,
        json!([{"op":"update_node","id":"master","props":{"name":"Dependency changed"}}]),
        false,
    );
    assert_eq!(master["committedRevision"], 4);
    let conflict = send(
        "conflict",
        3,
        json!([{"op":"update_node","id":"r0","props":{"name":"Rejected"}}]),
        true,
    );
    assert_eq!(conflict["error"]["code"], "REVISION_CONFLICT");
    let old = pair.on.reader_snapshot(&pair.project).unwrap();
    assert!(old.from_memory);
    let failed = send(
        "sql-fail",
        4,
        json!([{"op":"update_node","id":"r0","props":{"name":"fail"}}]),
        true,
    );
    assert!(failed.get("error").is_some());
    assert!(node_memory_shadow_stats().write_aborts > start.write_aborts);
    assert_eq!(old.shared.snapshot.revision, 4);
    assert_eq!(
        old.shared.scoped().unwrap().node("r0").unwrap().props["name"],
        "Allowed stale"
    );
    let after = pair.on.snapshot(&pair.project).unwrap();
    assert_eq!(after.revision, 4);
    assert_eq!(
        after.nodes.iter().find(|n| n.id == "r0").unwrap().props["name"],
        "Allowed stale"
    );
    // Explicit snapshot APIs must still materialize the complete authored model.
    let shared = pair.on.shared_snapshot(&pair.project).unwrap();
    assert_eq!(
        serde_json::to_value(shared.snapshot.as_ref()).unwrap(),
        serde_json::to_value(&after).unwrap()
    );
    assert!(!after.nodes.is_empty());
    pair.assert_rows(0);
    assert_eq!(
        send(
            "recovered",
            4,
            json!([{"op":"update_node","id":"r0","props":{"width":22}}]),
            true
        )["committedRevision"],
        5
    );
    pair.assert_rows(1);
}
