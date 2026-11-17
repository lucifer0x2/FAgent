use super::*;
use serde_json::json;

fn ours(ops: Value, changed: &[&str], gone: &[&str]) -> Writes {
    let changed: Vec<String> = changed.iter().map(|s| s.to_string()).collect();
    batch_writes(&ops, &changed, |id| !gone.contains(&id))
}

fn run(base: u64, commits: &[(u64, Value)], ours: &Writes) -> BatchResult {
    let mut result = BatchResult::default();
    report(
        &mut result,
        base,
        base + commits.len() as u64,
        commits,
        ours,
    );
    result
}

#[test]
fn reports_only_overwritten_keys_with_latest_revision() {
    let commits = [
        (
            3,
            json!([{"op":"update_node","id":"a","props":{"fill":"#000","width":1}}]),
        ),
        (
            4,
            json!({"scopeVersion":1,"operations":[{"op":"update_node","id":"a","props":{"fill":"#fff"}},{"op":"reorder_node","id":"b","index":0}]}),
        ),
    ];
    let ops = json!([{"op":"update_node","id":"a","props":{"fill":"#f00","height":2}},
        {"op":"reparent_node","id":"b","parentId":"p"},{"op":"update_node","id":"c","props":{"x":1}}]);
    let result = run(2, &commits, &ours(ops, &["a", "b", "c", "sibling"], &[]));
    let pairs: Vec<_> = result
        .concurrent
        .unwrap()
        .into_iter()
        .map(|w| (w.node_id, w.key, w.revision))
        .collect();
    assert_eq!(
        pairs,
        [
            ("a".into(), "fill".into(), 4),
            ("b".into(), "placement".into(), 4)
        ]
    );
    assert_eq!(result.concurrent_total, Some(2));
    assert!(!result.concurrent_unknown);
}

#[test]
fn deletes_match_every_key_and_created_ids_are_not_deletes() {
    let commits = [(
        6,
        json!([{"op":"update_node","id":"child","props":{"text":"x"}},
        {"op":"create_note","id":"n1","nodeId":"child","text":"t"}]),
    )];
    let ops = json!([{"op":"delete_subtree","id":"root"},{"op":"create_node","id":"tmp","kind":"frame","parentId":"p","props":{}},
        {"op":"delete_subtree","id":"tmp"}]);
    let writes = ours(
        ops,
        &["root", "child", "n1", "tmp"],
        &["root", "child", "n1", "tmp"],
    );
    assert!(
        !writes.contains(&("tmp".into(), "deleted".into()))
            || writes.contains(&("tmp".into(), "created".into()))
    );
    let result = run(5, &commits, &writes);
    let keys: Vec<_> = result
        .concurrent
        .unwrap()
        .into_iter()
        .map(|w| (w.node_id, w.key))
        .collect();
    assert_eq!(
        keys,
        [
            ("child".into(), "text".into()),
            ("n1".into(), "note".into())
        ]
    );
}

#[test]
fn incomplete_history_is_unknown_and_current_base_reports_nothing() {
    let writes = ours(
        json!([{"op":"update_node","id":"a","props":{"x":1}}]),
        &["a"],
        &[],
    );
    // A gap (retention) or a cut-off value (size cap) makes history unknown.
    let gap = [(5, json!([]))];
    let mut result = BatchResult::default();
    report(&mut result, 3, 5, &gap, &writes);
    assert!(result.concurrent_unknown && result.concurrent.is_none());
    let capped = [(4, Value::Null)];
    assert!(run(3, &capped, &writes).concurrent_unknown);
    let current = run(3, &[], &writes);
    assert_eq!(current, BatchResult::default());
}

fn import_operation(font: Value) -> Value {
    json!({"op":"import_asset","asset":{
        "id":"asset_imported","contentHash":"a".repeat(64),"mediaType":"font/ttf",
        "byteSize":100,"relativeKey":"a".repeat(64),"width":null,"height":null,"status":"ready"
    },"font":font})
}

#[test]
fn resource_import_arrays_and_envelopes_preserve_totals_and_latest_revisions() {
    let props: serde_json::Map<String, Value> =
        (0..25).map(|i| (format!("key{i:02}"), json!(1))).collect();
    let op = json!([{"op":"update_node","id":"a","props":props}]);
    let writes = ours(op.clone(), &["a"], &[]);
    for font in [
        Value::Null,
        json!({"id":"uploaded","family":"Uploaded","weight":400,"style":"normal","license":"OFL-1.1"}),
    ] {
        let import = json!([import_operation(font)]);
        for history in [
            import.clone(),
            json!({"scopeVersion":1,"operations":import}),
        ] {
            let commits = [
                (2, op.clone()),
                (3, history),
                (
                    4,
                    json!([{"op":"update_node","id":"a","props":{"key00":2}}]),
                ),
            ];
            let result = run(1, &commits, &writes);
            assert!(!result.concurrent_unknown);
            assert_eq!(result.concurrent_total, Some(25));
            let pairs = result.concurrent.unwrap();
            assert_eq!(pairs.len(), 20);
            assert_eq!(pairs[0].revision, 4);
            assert_eq!(pairs[0].key, "key00");
            assert!(pairs[1..].iter().all(|pair| pair.revision == 2));
        }
    }
}

#[test]
fn unknown_and_malformed_import_history_stays_unknown() {
    let writes = ours(
        json!([{"op":"update_node","id":"a","props":{"x":1}}]),
        &["a"],
        &[],
    );
    let valid = import_operation(Value::Null);
    let mut bad_font = valid.clone();
    bad_font["font"] = json!({"weight":"invalid"});
    let mut bad_asset = valid.clone();
    bad_asset["asset"]["byteSize"] = json!("invalid");
    let mut extra = valid.clone();
    extra["unknown"] = json!(true);
    for history in [
        Value::Null,
        json!({"future":{}}),
        json!({"importAsset":{}}),
        json!([{"op":"future_op"}]),
        json!([{"op":"import_asset"}]),
        json!([bad_font]),
        json!([bad_asset]),
        json!([extra]),
        // A multi-asset import is known history; mixing it with a batch op is not.
        json!([valid.clone(), {"op":"update_node","id":"a","props":{"x":2}}]),
    ] {
        for wrapped in [
            history.clone(),
            json!({"scopeVersion":1,"operations":history}),
        ] {
            let result = run(3, &[(4, wrapped)], &writes);
            assert!(result.concurrent_unknown);
            assert!(result.concurrent.is_none() && result.concurrent_total.is_none());
        }
    }
    let two = json!([valid.clone(), valid]);
    assert!(!run(3, &[(4, two)], &writes).concurrent_unknown);
    let oversized: Vec<_> = (1..=1001).map(|revision| (revision, json!([]))).collect();
    assert!(run(0, &oversized, &writes).concurrent_unknown);
}

#[test]
fn strict_requires_a_base_and_checks_are_mode_dependent() {
    let request = |expected: Option<u64>, strict| crate::BatchRequest {
        project_id: "p".into(),
        expected_revision: expected,
        strict,
        idempotency_key: "k".into(),
        operations: vec![],
    };
    assert_eq!(
        Base::of(&request(None, true)).unwrap_err().code,
        "VALIDATION_FAILED"
    );
    let lww = Base::of(&request(Some(3), false)).unwrap();
    assert!(lww.check(5, false).is_ok() && lww.check(2, false).is_err());
    let strict = Base::of(&request(Some(3), true)).unwrap();
    assert_eq!(
        strict.check(5, false).unwrap_err().code,
        "REVISION_CONFLICT"
    );
    assert!(strict.check(5, true).is_ok());
    assert_eq!(strict.history_after(3, true), Some(3));
    assert_eq!(lww.history_after(3, false), None);
}
