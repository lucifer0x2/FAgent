//! Bounded I4 smoke, reused by the deferred disposable-PostgreSQL run.
use super::*;

fn smoke(pair: &Pair) {
    pair.seed();
    let send = |key: &str, operations: Value, strict: bool, pages: Option<Value>| {
        let mut request = json!({"projectId":pair.project,"expectedRevision":1,"idempotencyKey":key,"operations":operations,"strict":strict});
        if let Some(pages) = pages {
            request["pageIds"] = pages;
        }
        pair.both(&request, false)
    };
    let result = send(
        "prefix",
        json!([
            {"op":"update_node","id":"r0","props":{"width":false}},
            {"op":"update_node","id":"r0","props":{"width":30}}
        ]),
        false,
        None,
    );
    assert_eq!(result["error"]["opIndex"], 0);
    let result = send(
        "local",
        json!([{"op":"update_node","id":"r0","props":{"name":"local"}}]),
        true,
        Some(json!(["pg"])),
    );
    assert_eq!(result["committedRevision"], 2);
    let result = send(
        "uncovered",
        json!([{"op":"update_node","id":"r0","props":{"width":20}}]),
        false,
        None,
    );
    assert_eq!(result["committedRevision"], 3);
    assert!(result.get("concurrent").is_some());
    let result = send(
        "cross-page",
        json!([
            {"op":"create_node","id":"child","kind":"rectangle","parentId":"master","props":{"width":10,"height":10}},
            {"op":"update_node","id":"inst","props":{"width":"hug"}}
        ]),
        false,
        None,
    );
    assert_eq!(result["committedRevision"], 4);
    let result = send(
        "cross-reject",
        json!([{"op":"update_node","id":"child","props":{"width":"fill"}}]),
        false,
        None,
    );
    assert!(result.get("error").is_some());
    let result = send(
        "placement",
        json!([
            {"op":"create_node","id":"extra","kind":"rectangle","parentId":"f0","index":1,"props":{"width":10,"height":10}},
            {"op":"reparent_node","id":"extra","parentId":"f1","index":0},
            {"op":"reorder_node","id":"r0","index":0}
        ]),
        false,
        None,
    );
    assert_eq!(result["committedRevision"], 5);
    let replay = send(
        "placement",
        json!([
            {"op":"create_node","id":"extra","kind":"rectangle","parentId":"f0","index":1,"props":{"width":10,"height":10}},
            {"op":"reparent_node","id":"extra","parentId":"f1","index":0},
            {"op":"reorder_node","id":"r0","index":0}
        ]),
        false,
        None,
    );
    assert_eq!(replay["replayed"], true);
    pair.assert_rows(0);
}

#[test]
fn sqlite_i4_delta_smoke_and_db_failure_rollback() {
    let temp = tempfile::tempdir().unwrap();
    let pair = sqlite_pair(temp.path());
    // Install before admission, so the DB identity remains current when the
    // staged write fails. Both engines return the same public storage error.
    for name in ["off", "on"] {
        let db = rusqlite::Connection::open(temp.path().join(name).join("diff/db.sql")).unwrap();
        db.execute_batch("CREATE TRIGGER i4_fail BEFORE UPDATE ON nodes WHEN json_extract(NEW.props_json,'$.name')='fail' BEGIN SELECT RAISE(ABORT,'i4 forced failure'); END;").unwrap();
    }
    smoke(&pair);
    let old = pair.on.snapshot(&pair.project).unwrap();
    let result = pair.both(&json!({"projectId":pair.project,"idempotencyKey":"db-fail","operations":[{"op":"update_node","id":"r0","props":{"name":"fail"}}]}), false);
    assert!(result.get("error").is_some());
    assert_eq!(
        pair.on.snapshot(&pair.project).unwrap().revision,
        old.revision
    );
    pair.assert_rows(1);
    let result = pair.both(&json!({"projectId":pair.project,"idempotencyKey":"after-fail","operations":[{"op":"update_node","id":"r0","props":{"width":22}}]}), false);
    assert_eq!(result["committedRevision"], old.revision + 1);
    pair.assert_rows(2);
}

#[test]
#[ignore = "deferred: disposable PostgreSQL via scripts/task19-check.sh"]
fn postgres_i4_delta_smoke() {
    smoke(&postgres_pair("i4-smoke"));
}
