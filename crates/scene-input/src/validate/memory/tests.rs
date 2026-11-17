//! Small deterministic delta/reference checks; generated differential is deferred.
use super::*;
use crate::Snapshot;
use serde_json::{json, Value};

fn fixture() -> Snapshot {
    serde_json::from_value(json!({
        "schemaVersion":4,"projectId":"p","slug":"p","name":"p","revision":1,
        "retention":{"oldestCommitRevision":0,"idempotencyPolicy":"bounded"},
        "tokens":[],"styles":[],"assets":[],"fonts":[],"notes":[],
        "nodes":[
            {"id":"p","parentId":null,"sortKey":0,"kind":"page","props":{}},
            {"id":"q","parentId":null,"sortKey":1,"kind":"page","props":{}},
            {"id":"f","parentId":"p","sortKey":0,"kind":"frame","props":{"width":400,"height":400}},
            {"id":"m","parentId":"q","sortKey":0,"kind":"frame","props":{"width":40,"height":40,"component":{"name":"M"}}},
            {"id":"r","parentId":"m","sortKey":0,"kind":"rectangle","props":{"width":10,"height":10}},
            {"id":"i","parentId":"f","sortKey":0,"kind":"instance","props":{"componentId":"m"}},
            {"id":"g","parentId":"f","sortKey":1,"kind":"group","props":{"width":100,"height":100}},
            {"id":"u","parentId":"g","sortKey":0,"kind":"rectangle","props":{"width":10,"height":10}}
        ]
    })).unwrap()
}
fn ops(value: Value) -> Vec<Operation> {
    serde_json::from_value(value).unwrap()
}

fn advance(before: &ReadView, operations: &[Operation]) -> (ReadView, Vec<String>, Result<()>) {
    let mut candidate = Candidate::from_snapshot(&before.to_snapshot());
    let mut changed = BTreeSet::new();
    for (i, op) in operations.iter().enumerate() {
        candidate
            .apply(op, &mut changed)
            .map_err(|e| e.at(i))
            .unwrap();
    }
    candidate.prune_links(&mut changed);
    let reference = candidate.validate();
    let after = before.advance(
        before.metadata().revision + 1,
        changed
            .iter()
            .map(|id| (id.as_str(), candidate.nodes.get(id))),
        changed
            .iter()
            .filter(|id| before.note(id).is_some())
            .map(|id| (id.as_str(), candidate.notes.get(id))),
        None,
    );
    (after, changed.into_iter().collect(), reference)
}

#[test]
fn memory_delta_master_users_and_dependencies_are_local_and_cross_page() {
    let before = ReadView::new(fixture());
    let operations = ops(json!([{"op":"update_node","id":"r","props":{"fill":"#ff0000"}}]));
    let (after, changed, reference) = advance(&before, &operations);
    reference.unwrap();
    let work = validate_delta(&before, &after, &operations, &changed)
        .unwrap()
        .unwrap();
    assert!(work.affected_ids.contains("i"));
    assert!(!work.affected_ids.contains("u"));
    assert!(work.stats.affected_nodes < after.len());
    assert_eq!(work.expanded.unwrap()["i/r"].props["fill"], "#ff0000");
    assert_eq!(
        before.validation.get().unwrap().total,
        after.validation.get().unwrap().total
    );

    let mut snapshot = fixture();
    snapshot
        .nodes
        .iter_mut()
        .find(|n| n.id == "i")
        .unwrap()
        .props["width"] = json!("hug");
    let before = ReadView::new(snapshot);
    let operations = ops(json!([{"op":"update_node","id":"r","props":{"width":"fill"}}]));
    let (rejected, changed, reference) = advance(&before, &operations);
    assert!(reference.is_err());
    let actual = validate_delta(&before, &rejected, &operations, &changed).map(|_| ());
    assert_eq!(
        serde_json::to_value(actual.err()).unwrap(),
        serde_json::to_value(reference.err()).unwrap()
    );
}

#[test]
fn memory_delta_replays_invalid_intermediate_props_with_operation_index() {
    let before = ReadView::new(fixture());
    let operations = ops(json!([
        {"op":"update_node","id":"r","props":{"width":false}},
        {"op":"update_node","id":"r","props":{"width":20}}
    ]));
    let mut final_row = before.node("r").unwrap().clone();
    final_row.props["width"] = json!(20);
    let after = before.advance(2, [("r", Some(&final_row))], [], None);
    let actual = validate_delta(&before, &after, &operations, &["r".into()])
        .err()
        .unwrap();
    let mut full = Candidate::from_snapshot(&fixture());
    let expected = full
        .apply(&operations[0], &mut BTreeSet::new())
        .unwrap_err()
        .at(0);
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(after.validation.get().is_none());
}

#[test]
fn memory_delta_expansion_budget_includes_unaffected_instances() {
    let before = ReadView::new(fixture());
    // Synthetic near-cap certificate isolates the project-wide accounting edge
    // without generating a large fixture. Editing u's subtree sees no instances.
    before
        .validation
        .set(Certificate {
            extra: Map::default(),
            total: MAX_NODES,
        })
        .ok()
        .unwrap();
    let operations = ops(
        json!([{"op":"create_node","id":"child","kind":"rectangle","parentId":"g","props":{"width":2,"height":2}}]),
    );
    let (after, changed, _) = advance(&before, &operations);
    assert_eq!(
        validate_delta(&before, &after, &operations, &changed)
            .err()
            .unwrap()
            .code,
        "INSTANCE_LIMIT"
    );
}

#[test]
fn memory_delta_unvalidated_legacy_load_cannot_grant_partial_trust() {
    let mut snapshot = fixture();
    snapshot
        .nodes
        .iter_mut()
        .find(|n| n.id == "u")
        .unwrap()
        .props["width"] = json!(false);
    let before = ReadView::new(snapshot);
    let operations = ops(json!([{"op":"update_node","id":"r","props":{"width":20}}]));
    let (after, changed, _) = advance(&before, &operations);
    assert!(validate_delta(&before, &after, &operations, &changed)
        .unwrap()
        .is_none());
    assert!(before.validation.get().is_none());
}

#[test]
fn memory_delta_nested_master_chain_updates_transitive_users_and_budget_twice() {
    let mut snapshot = fixture();
    for node in [
        json!({"id":"outer-master","parentId":"q","sortKey":1,"kind":"frame","props":{"width":100,"height":100,"component":{"name":"Outer"}}}),
        json!({"id":"nested","parentId":"outer-master","sortKey":0,"kind":"instance","props":{"componentId":"m"}}),
        json!({"id":"outer-user","parentId":"f","sortKey":2,"kind":"instance","props":{"componentId":"outer-master"}}),
    ] {
        snapshot.nodes.push(serde_json::from_value(node).unwrap());
    }
    let mut before = ReadView::new(snapshot);
    for (index, operations) in [
        ops(json!([{"op":"update_node","id":"r","props":{"fill":"#abcdef"}}])),
        ops(json!([{"op":"create_node","id":"second","kind":"rectangle","parentId":"m","props":{"width":2,"height":2}}])),
    ].into_iter().enumerate() {
        let (after, changed, reference) = advance(&before, &operations);
        reference.unwrap();
        let work = validate_delta(&before, &after, &operations, &changed).unwrap().unwrap();
        for id in ["i", "nested", "outer-user"] { assert!(work.affected_ids.contains(id), "{id}"); }
        let placed = work.expanded.unwrap();
        assert_eq!(placed["outer-user/nested/r"].props["fill"], "#abcdef");
        if index == 1 { assert!(placed.contains_key("outer-user/nested/second")); }
        let full = crate::instances::expand(&after.to_snapshot().nodes).unwrap();
        assert_eq!(after.validation.get().unwrap().total, full.len());
        before = after;
    }
}
