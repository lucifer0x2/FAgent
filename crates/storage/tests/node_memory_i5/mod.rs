//! Bounded end-to-end strict-page conflict and SQL rollback oracle, with readers that
//! keep the memory view they were handed across later commits.
use super::*;

#[test]
fn i5_sqlite_stale_strict_dispatch_rollback_and_retained_reader() {
    let temp = tempfile::tempdir().unwrap();
    let pair = sqlite_pair(temp.path());
    for name in ["warm", "cold"] {
        let db = rusqlite::Connection::open(temp.path().join(name).join("diff/db.sql")).unwrap();
        db.execute_batch("CREATE TRIGGER i5_fail BEFORE UPDATE ON nodes WHEN json_extract(NEW.props_json,'$.name')='fail' BEGIN SELECT RAISE(ABORT,'i5 forced failure'); END;").unwrap();
    }
    // Commits leave the project resident: the next read is served from memory.
    let hot_reader = || {
        let read = pair.warm.reader_snapshot(&pair.project).unwrap();
        assert!(read.from_memory);
        read
    };
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
    let retained = hot_reader();
    let edit = json!([{"op":"update_node","id":"r0","props":{"name":"Allowed stale"}}]);
    let result = send("stale", 1, edit.clone(), true);
    assert_eq!(result["committedRevision"], 3);
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
    let old = hot_reader();
    let failed = send(
        "sql-fail",
        4,
        json!([{"op":"update_node","id":"r0","props":{"name":"fail"}}]),
        true,
    );
    assert!(failed.get("error").is_some());
    // The failed persist dropped the staged batch and evicted the project.
    assert!(pair.warm.memory_snapshot(&pair.project).is_none());
    assert_eq!(pair.warm.memory_stats().write_aborts, 1);
    assert_eq!(old.shared.snapshot.revision, 4);
    assert_eq!(
        old.shared.scoped().unwrap().node("r0").unwrap().props["name"],
        "Allowed stale"
    );
    let after = pair.warm.snapshot(&pair.project).unwrap();
    assert_eq!(after.revision, 4);
    assert_eq!(
        after.nodes.iter().find(|n| n.id == "r0").unwrap().props["name"],
        "Allowed stale"
    );
    // Explicit snapshot APIs must still materialize the complete authored model.
    let shared = pair.warm.shared_database(&pair.project).unwrap();
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
