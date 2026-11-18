//! `guard` and the engine footprint against the candidate path they replace
//! (`validated_candidate`, `BatchScope`, `scoped::check`, `concurrent::fill`): the
//! same changed ids, footprint, guard errors and results for every operation class,
//! guarded page set and history shape, including stale bases and legacy history.
use super::guard;
use crate::scoped::{self, parity, BatchScope};
use crate::*;
use fagent_node_memory::{Begin, MemoryConfig, NodeMemory, SystemClock};

type Public<T> = std::result::Result<T, Value>;

fn public<T>(result: Result<T>) -> Public<T> {
    result.map_err(|e| serde_json::to_value(e).unwrap())
}

fn ops(value: Value) -> Vec<Operation> {
    serde_json::from_value(value).unwrap()
}

/// The seeded project after one unrelated commit (revision 2), and the history
/// envelopes of two commits at revision 2: that unrelated one and one on `r1`.
fn fixture() -> (Snapshot, Value, Value) {
    let initial = parity::seed();
    let envelope = |operations: Vec<Operation>| {
        let (candidate, changed, _) =
            crate::batch::validated_candidate(&initial, &operations).unwrap();
        let scope = BatchScope::new(&initial, &candidate, &changed).unwrap();
        let body = json!({"projectId":initial.project_id,"expectedRevision":1,"idempotencyKey":"h","operations":operations});
        let history =
            scoped::envelope_with_scopes(scope.footprint, body.clone(), body["operations"].clone())
                .unwrap();
        (candidate, history)
    };
    let unrelated = ops(json!([
        {"op":"upsert_token","token":{"id":"unused","name":"Unused","type":"number","value":1}}
    ]));
    let (candidate, unrelated) = envelope(unrelated);
    let (_, overlapping) = envelope(ops(
        json!([{"op":"update_node","id":"r1","props":{"name":"Theirs"}}]),
    ));
    let mut before = parity::snapshot(&initial, &candidate);
    before.revision = 2;
    (before, unrelated, overlapping)
}

fn cases() -> Vec<Value> {
    vec![
        json!([{"op":"update_node","id":"r1","props":{"name":"Allowed"}}]),
        json!([{"op":"update_node","id":"r1","props":{"name":"Same"}},{"op":"update_node","id":"r1","props":{"name":"Same"}}]),
        json!([{"op":"update_node","id":"r1","props":{"fill":"#00ff00"}}]),
        json!([{"op":"update_node","id":"r2","props":{"width":30}}]),
        json!([{"op":"create_node","id":"new","kind":"rectangle","parentId":"f1","props":{"width":5,"height":5}}]),
        json!([{"op":"create_node","id":"new","kind":"rectangle","parentId":"f1","index":0,"props":{"width":5,"height":5}}]),
        json!([{"op":"create_node","id":"append","kind":"frame","parentId":"p1","props":{"width":10,"height":10}}]),
        json!([{"op":"create_node","id":"append","kind":"frame","parentId":"p1","index":1,"props":{"width":10,"height":10}}]),
        json!([{"op":"create_node","id":"first","kind":"frame","parentId":"empty","props":{"width":10,"height":10}}]),
        json!([{"op":"create_node","id":"tail-page","kind":"page","parentId":null,"props":{"name":"Tail"}},{"op":"create_node","id":"nf","kind":"frame","parentId":"tail-page","props":{"width":10,"height":10}}]),
        json!([{"op":"create_node","id":"front","kind":"page","parentId":null,"index":0,"props":{"name":"Front"}}]),
        json!([{"op":"reparent_node","id":"r1","parentId":"f2"}]),
        json!([{"op":"reparent_node","id":"r1","parentId":"tail"}]),
        json!([{"op":"reparent_node","id":"tail","parentId":"f1"}]),
        json!([{"op":"reorder_node","id":"tail","index":0}]),
        json!([{"op":"reorder_node","id":"f1","index":0}]),
        json!([{"op":"reorder_node","id":"p2","index":0}]),
        json!([{"op":"delete_subtree","id":"r1"}]),
        json!([{"op":"delete_subtree","id":"r2"}]),
        json!([{"op":"delete_subtree","id":"f2"}]),
        json!([{"op":"delete_subtree","id":"tail"}]),
        json!([{"op":"delete_subtree","id":"p2"}]),
        json!([{"op":"update_node","id":"p1","props":{"name":"Page"}}]),
        json!([{"op":"update_node","id":"f1","props":{"flowStart":"global"}}]),
        json!([{"op":"update_node","id":"r1","props":{"links":[{"to":"tail"}]}}]),
        json!([{"op":"update_node","id":"instance","props":{"theme":"night"}}]),
        json!([{"op":"update_node","id":"label","props":{"text":"Master changed"}}]),
        json!([{"op":"update_node","id":"label2","props":{"text":"Direct changed"}}]),
        json!([{"op":"upsert_token","token":{"id":"base","name":"Base","type":"color","value":"#ffffff"}}]),
        json!([{"op":"upsert_style","style":{"id":"fill","name":"Fill","kind":"fill","value":"#123456"}}]),
        json!([{"op":"create_note","id":"n2","nodeId":"r1","text":"x"}]),
        json!([{"op":"create_note","id":"n2","nodeId":"r2","text":"x"}]),
        json!([{"op":"reply_note","id":"note","text":"reply"}]),
        json!([{"op":"update_note","id":"note","text":"Edited"}]),
        json!([{"op":"update_note","id":"note","nodeId":"r1"}]),
        json!([{"op":"delete_note","id":"note"}]),
        json!([{"op":"create_note","id":"n2","nodeId":"r1","text":"x"},{"op":"delete_subtree","id":"r1"}]),
        // Rejected by validation: per-operation and end-of-batch.
        json!([{"op":"update_node","id":"missing","props":{"name":"No"}}]),
        json!([{"op":"update_node","id":"r1","props":{"width":-1}}]),
        json!([{"op":"update_node","id":"r1","props":{"fill":{"tokenId":"missing"}}}]),
        json!([{"op":"update_node","id":"r1","props":{"links":[{"to":"r2"}]}}]),
    ]
}

fn page_sets() -> Vec<Vec<String>> {
    let sets: [&[&str]; 9] = [
        &[],
        &["missing"],
        &["p1", "p1"],
        &["p1"],
        &["p2"],
        &["library"],
        &["empty"],
        &["p1", "p2"],
        &["p1", "p2", "library", "empty", "tail-page"],
    ];
    sets.iter()
        .map(|set| set.iter().map(|p| p.to_string()).collect())
        .collect()
}

#[test]
fn guard_footprint_and_result_match_the_candidate_path() {
    let (before, unrelated, overlapping) = fixture();
    let engine = NodeMemory::new(MemoryConfig::default(), Arc::new(SystemClock::new()));
    let project = engine.load(before.clone()).unwrap();
    let legacy = json!([{"op":"update_node","id":"f2","props":{"name":"Legacy"}}]);
    // (expected revision, history after it)
    let histories: Vec<(u64, Vec<(u64, Value)>)> = vec![
        (2, vec![]),
        (1, vec![(2, unrelated)]),
        (1, vec![(2, overlapping)]),
        (1, vec![(2, legacy)]),
        (0, vec![]),
        (3, vec![]),
    ];
    let (mut compared, mut accepted) = (0, 0);
    for case in cases() {
        let operations = ops(case.clone());
        let reference = public(crate::batch::validated_candidate(&before, &operations));
        let staged = public(project.begin(before.revision, &operations));
        let (candidate, changed, prepared) = match (reference, staged) {
            (Err(expected), Err(actual)) => {
                assert_eq!(actual, expected, "validation error, case={case}");
                continue;
            }
            (Ok((candidate, changed, _)), Ok(Begin::Ready(prepared))) => {
                (candidate, changed, prepared)
            }
            (reference, staged) => panic!(
                "case={case}: reference ok={} engine ok={}",
                reference.is_ok(),
                staged.is_ok()
            ),
        };
        assert_eq!(prepared.changed_ids(), changed, "changed ids, case={case}");
        let scope = BatchScope::new(&before, &candidate, &changed).unwrap();
        assert_eq!(
            prepared.footprint(),
            &scope.footprint,
            "footprint, case={case}"
        );
        for pages in page_sets() {
            for (expected, commits) in &histories {
                let request = BatchRequest {
                    project_id: before.project_id.clone(),
                    expected_revision: Some(*expected),
                    strict: true,
                    idempotency_key: "guard".into(),
                    operations: operations.clone(),
                };
                let mut work = crate::batch::validated_candidate(&before, &operations)
                    .unwrap()
                    .2;
                let old = public(scoped::check(
                    &before, &candidate, &request, &pages, commits, &scope, &mut work,
                ));
                let new = public(guard::check(&prepared, &request, &pages, commits));
                assert_eq!(
                    new, old,
                    "guard, case={case} pages={pages:?} base={expected}"
                );
                accepted += usize::from(new.is_ok());
                let operations = serde_json::to_value(&request.operations).unwrap();
                let mut old = BatchResult {
                    project_id: before.project_id.clone(),
                    committed_revision: before.revision + 1,
                    changed_ids: changed.clone(),
                    render_status: "pending".into(),
                    ..Default::default()
                };
                let base = concurrent::Base::of(&request).unwrap();
                concurrent::fill(
                    &mut old,
                    base,
                    before.revision,
                    commits,
                    &operations,
                    &candidate,
                );
                let new = guard::result(&prepared, &request, commits, &operations);
                assert_eq!(new, old, "result, case={case} base={expected}");
                compared += 1;
            }
        }
        // Dropping the prepared batch rolls it back for the next case.
        drop(prepared);
        assert_eq!(project.view().metadata().revision, before.revision);
    }
    assert!(compared > 1500, "{compared} comparisons");
    assert!(accepted > 50, "{accepted} accepted guards");
}
