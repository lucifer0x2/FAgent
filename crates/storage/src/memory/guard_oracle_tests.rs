//! Capture step for `guard_golden.json`: the candidate path (`validated_candidate`,
//! `BatchScope`, `scoped::check`, `concurrent::fill`) produces the pinned matrix on
//! its own fixture. Removed with that path.
use super::guard_tests::{self as golden, Outcome};
use crate::scoped::{self, parity, BatchScope};
use crate::*;

const GOLDEN: &str = include_str!("guard_golden.json");
const GOLDEN_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/memory/guard_golden.json");

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
    let unrelated = golden::ops(json!([
        {"op":"upsert_token","token":{"id":"unused","name":"Unused","type":"number","value":1}}
    ]));
    let (candidate, unrelated) = envelope(unrelated);
    let (_, overlapping) = envelope(golden::ops(
        json!([{"op":"update_node","id":"r1","props":{"name":"Theirs"}}]),
    ));
    let mut before = parity::snapshot(&initial, &candidate);
    before.revision = 2;
    (before, unrelated, overlapping)
}

/// Entities sorted by id, so both fixtures compare regardless of order.
fn normalized(snapshot: &Snapshot) -> Value {
    let mut value = serde_json::to_value(snapshot).unwrap();
    for field in ["nodes", "tokens", "styles", "fonts", "assets", "notes"] {
        value[field]
            .as_array_mut()
            .unwrap()
            .sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    }
    value
}

#[test]
fn the_candidate_path_reproduces_the_guard_golden_matrix() {
    let (before, unrelated, overlapping) = fixture();
    let (engine_before, engine_unrelated, engine_overlapping) = golden::fixture();
    // Each seed is a new project; everything but its id must match.
    let (mut a, mut b) = (normalized(&engine_before), normalized(&before));
    a["projectId"] = json!(null);
    b["projectId"] = json!(null);
    assert_eq!(a, b, "fixture");
    for (engine, oracle) in [
        (&engine_unrelated, &unrelated),
        (&engine_overlapping, &overlapping),
    ] {
        assert_eq!(engine["scopes"], oracle["scopes"]);
        assert_eq!(engine["operations"], oracle["operations"]);
    }
    let histories = golden::histories(unrelated, overlapping);
    let mut outcomes = Vec::new();
    for case in golden::cases() {
        let operations = golden::ops(case);
        let (candidate, changed) =
            match golden::public(crate::batch::validated_candidate(&before, &operations)) {
                Err(error) => {
                    outcomes.push(Outcome::Rejected(error));
                    continue;
                }
                Ok((candidate, changed, _)) => (candidate, changed),
            };
        let scope = BatchScope::new(&before, &candidate, &changed).unwrap();
        let guards = golden::page_sets()
            .iter()
            .map(|pages| {
                histories
                    .iter()
                    .map(|(expected, commits)| {
                        let request = golden::request(&before, *expected, &operations);
                        let mut work = crate::batch::validated_candidate(&before, &operations)
                            .unwrap()
                            .2;
                        golden::public(scoped::check(
                            &before, &candidate, &request, pages, commits, &scope, &mut work,
                        ))
                    })
                    .collect()
            })
            .collect();
        let canonical = serde_json::to_value(&operations).unwrap();
        let results = histories
            .iter()
            .map(|(expected, commits)| {
                let request = golden::request(&before, *expected, &operations);
                let mut result = BatchResult {
                    project_id: before.project_id.clone(),
                    committed_revision: before.revision + 1,
                    changed_ids: changed.clone(),
                    render_status: "pending".into(),
                    ..Default::default()
                };
                let base = concurrent::Base::of(&request).unwrap();
                concurrent::fill(
                    &mut result,
                    base,
                    before.revision,
                    commits,
                    &canonical,
                    &candidate,
                );
                result
            })
            .collect();
        outcomes.push(Outcome::Staged {
            changed,
            footprint: scope.footprint.clone(),
            guards,
            results,
        });
    }
    let actual = golden::golden(&before, &outcomes);
    if std::env::var_os("FAGENT_CAPTURE_ORACLE_GUARD_GOLDEN").is_some() {
        std::fs::write(GOLDEN_PATH, golden::format(&actual)).unwrap();
        return;
    }
    let expected: Value = serde_json::from_str(GOLDEN).unwrap();
    assert_eq!(actual, expected);
}
