//! The strict page guard, the engine footprint and last-writer-wins results for
//! every operation class, guarded page set and history shape (current, old
//! unrelated, old overlapping, legacy without scope, gap, base ahead), pinned in
//! `guard_golden.json`. The fixture was captured from the candidate path these
//! replaced. After an intended change, regenerate it with
//! `FAGENT_UPDATE_GUARD_GOLDEN=1 cargo test -p fagent-storage guard_golden` and
//! review the diff.
use super::guard;
use crate::scoped::{self, parity};
use crate::*;
use fagent_node_memory::{Begin, MemoryConfig, NodeMemory, Prepared, SystemClock};

const GOLDEN: &str = include_str!("guard_golden.json");
const GOLDEN_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/memory/guard_golden.json");
const UPDATE: &str = "FAGENT_UPDATE_GUARD_GOLDEN";
const HISTORIES: [&str; 6] = [
    "current",
    "old unrelated",
    "old overlapping",
    "legacy without scope",
    "gap",
    "base ahead",
];

type Public<T> = std::result::Result<T, Value>;

fn public<T>(result: Result<T>) -> Public<T> {
    result.map_err(|e| serde_json::to_value(e).unwrap())
}

fn ops(value: Value) -> Vec<Operation> {
    serde_json::from_value(value).unwrap()
}

/// What one case produced: a validation rejection, or the staged batch's changed
/// ids and footprint, its guard outcome per page set and history, and its result
/// per history.
enum Outcome {
    Rejected(Value),
    Staged {
        changed: Vec<String>,
        footprint: BTreeSet<String>,
        guards: Vec<Vec<Public<()>>>,
        results: Vec<BatchResult>,
    },
}

fn engine() -> NodeMemory {
    NodeMemory::new(MemoryConfig::default(), Arc::new(SystemClock::new()))
}

fn ready(begin: Result<Begin<'_>>) -> Prepared<'_> {
    match begin.unwrap() {
        Begin::Ready(prepared) => prepared,
        Begin::Stale => panic!("stale base"),
    }
}

/// The seeded project after one unrelated commit (revision 2), and the history
/// envelopes of two commits on revision 1: that unrelated one and one on `r1`.
fn fixture() -> (Snapshot, Value, Value) {
    let initial = parity::seed();
    let engine = engine();
    let project = engine.load(initial.clone()).unwrap();
    let envelope = |operations: &[Operation], footprint: &BTreeSet<String>| {
        let body = json!({"projectId":initial.project_id,"expectedRevision":1,"idempotencyKey":"h","operations":operations});
        scoped::envelope_with_scopes(footprint.clone(), body.clone(), body["operations"].clone())
            .unwrap()
    };
    let theirs = ops(json!([{"op":"update_node","id":"r1","props":{"name":"Theirs"}}]));
    // Dropping the prepared batch rolls it back.
    let overlapping = envelope(&theirs, ready(project.begin(1, &theirs)).footprint());
    let unused = ops(json!([
        {"op":"upsert_token","token":{"id":"unused","name":"Unused","type":"number","value":1}}
    ]));
    let prepared = ready(project.begin(1, &unused));
    let unrelated = envelope(&unused, prepared.footprint());
    let _ = prepared.commit();
    let before = engine.to_snapshot(&initial.project_id).unwrap();
    assert_eq!(before.revision, 2);
    (before, unrelated, overlapping)
}

/// (expected revision, history after it), in `HISTORIES` order.
fn histories(unrelated: Value, overlapping: Value) -> Vec<(u64, Vec<(u64, Value)>)> {
    let legacy = json!([{"op":"update_node","id":"f2","props":{"name":"Legacy"}}]);
    vec![
        (2, vec![]),
        (1, vec![(2, unrelated)]),
        (1, vec![(2, overlapping)]),
        (1, vec![(2, legacy)]),
        (0, vec![]),
        (3, vec![]),
    ]
}

fn request(before: &Snapshot, expected: u64, operations: &[Operation]) -> BatchRequest {
    BatchRequest {
        project_id: before.project_id.clone(),
        expected_revision: Some(expected),
        strict: true,
        idempotency_key: "guard".into(),
        operations: operations.to_vec(),
    }
}

fn cases() -> Vec<Value> {
    vec![
        json!([{"op":"update_node","id":"r1","props":{"name":"Allowed"}}]),
        json!([{"op":"update_node","id":"r1","props":{"name":"Same"}},{"op":"update_node","id":"r1","props":{"name":"Same"}}]),
        json!([{"op":"update_node","id":"r1","props":{"fill":"#00ff00"}}]),
        json!([{"op":"update_node","id":"r1","props":{"x":-0.0}}]),
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

/// Every case staged in `NodeMemory` on the fixture, checked by `guard`.
fn memory_outcomes(before: &Snapshot, unrelated: Value, overlapping: Value) -> Vec<Outcome> {
    let engine = engine();
    let project = engine.load(before.clone()).unwrap();
    let histories = histories(unrelated, overlapping);
    let mut outcomes = Vec::new();
    for case in cases() {
        let operations = ops(case);
        let prepared = match public(project.begin(before.revision, &operations)) {
            Err(error) => {
                outcomes.push(Outcome::Rejected(error));
                continue;
            }
            Ok(Begin::Ready(prepared)) => prepared,
            Ok(Begin::Stale) => panic!("stale base"),
        };
        let guards = page_sets()
            .iter()
            .map(|pages| {
                histories
                    .iter()
                    .map(|(expected, commits)| {
                        let request = request(before, *expected, &operations);
                        public(guard::check(&prepared, &request, pages, commits))
                    })
                    .collect()
            })
            .collect();
        let canonical = serde_json::to_value(&operations).unwrap();
        let results = histories
            .iter()
            .map(|(expected, commits)| {
                let request = request(before, *expected, &operations);
                guard::result(&prepared, &request, commits, &canonical)
            })
            .collect();
        outcomes.push(Outcome::Staged {
            changed: prepared.changed_ids().to_vec(),
            footprint: prepared.footprint().clone(),
            guards,
            results,
        });
        // Dropping the prepared batch rolls it back for the next case.
        drop(prepared);
        assert_eq!(project.view().metadata().revision, before.revision);
    }
    outcomes
}

/// The golden document: distinct errors sorted once and referenced by index;
/// results keep only the fields that differ from a plain commit of the case.
fn golden(before: &Snapshot, outcomes: &[Outcome]) -> Value {
    let mut errors = BTreeMap::new();
    let mut collect = |error: &Value| {
        errors.insert(serde_json::to_string(error).unwrap(), error.clone());
    };
    for outcome in outcomes {
        match outcome {
            Outcome::Rejected(error) => collect(error),
            Outcome::Staged { guards, .. } => guards.iter().flatten().for_each(|guard| {
                if let Err(error) = guard {
                    collect(error)
                }
            }),
        }
    }
    let keys: Vec<_> = errors.keys().cloned().collect();
    let index = |error: &Value| {
        let key = serde_json::to_string(error).unwrap();
        json!(keys.iter().position(|k| *k == key).unwrap())
    };
    let cases = cases()
        .into_iter()
        .zip(outcomes)
        .map(|(operations, outcome)| match outcome {
            Outcome::Rejected(error) => json!({"operations":operations,"error":index(error)}),
            Outcome::Staged {
                changed,
                footprint,
                guards,
                results,
            } => {
                let plain = json!({"projectId":before.project_id,"committedRevision":before.revision + 1,
                    "changedIds":changed,"renderStatus":"pending","replayed":false});
                let results: Vec<_> = results
                    .iter()
                    .map(|result| {
                        let mut result = serde_json::to_value(result).unwrap();
                        let fields = result.as_object_mut().unwrap();
                        fields.retain(|key, value| plain.get(key) != Some(value));
                        result
                    })
                    .collect();
                let guards: Vec<Vec<_>> = guards
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|guard| guard.as_ref().err().map_or(Value::Null, index))
                            .collect()
                    })
                    .collect();
                json!({"operations":operations,"changedIds":changed,"footprint":footprint,
                    "results":results,"guards":guards})
            }
        })
        .collect::<Vec<_>>();
    json!({"histories":HISTORIES,"pageSets":page_sets(),"errors":errors.into_values().collect::<Vec<_>>(),
        "cases":cases})
}

/// One line per error and per case field, so a regenerated fixture diffs well.
fn format(golden: &Value) -> String {
    let line = |value: &Value| serde_json::to_string(value).unwrap();
    let items = |values: &Value, render: &dyn Fn(&Value) -> String| {
        values
            .as_array()
            .unwrap()
            .iter()
            .map(render)
            .collect::<Vec<_>>()
            .join(",\n")
    };
    let case = |case: &Value| {
        let fields = case.as_object().unwrap();
        let fields: Vec<_> = fields
            .iter()
            .map(|(key, value)| format!("      {}: {}", line(&json!(key)), line(value)))
            .collect();
        format!("    {{\n{}\n    }}", fields.join(",\n"))
    };
    format!(
        "{{\n  \"histories\": {},\n  \"pageSets\": {},\n  \"errors\": [\n{}\n  ],\n  \"cases\": [\n{}\n  ]\n}}\n",
        line(&golden["histories"]),
        line(&golden["pageSets"]),
        items(&golden["errors"], &|error| format!("    {}", line(error))),
        items(&golden["cases"], &case),
    )
}

#[test]
fn guard_golden_matrix_matches_the_memory_guard() {
    let (before, unrelated, overlapping) = fixture();
    let actual = golden(&before, &memory_outcomes(&before, unrelated, overlapping));
    if std::env::var_os(UPDATE).is_some() {
        std::fs::write(GOLDEN_PATH, format(&actual)).unwrap();
        return;
    }
    let expected: Value = serde_json::from_str(GOLDEN).unwrap();
    assert_eq!(actual["histories"], expected["histories"]);
    assert_eq!(actual["pageSets"], expected["pageSets"]);
    assert_eq!(actual["errors"], expected["errors"], "error table");
    let (actual_cases, expected_cases) = (
        actual["cases"].as_array().unwrap(),
        expected["cases"].as_array().unwrap(),
    );
    assert_eq!(actual_cases.len(), expected_cases.len());
    for (actual, expected) in actual_cases.iter().zip(expected_cases) {
        assert_eq!(actual, expected, "case={}", expected["operations"]);
    }
    let (mut compared, mut accepted) = (0, 0);
    for case in expected_cases {
        for row in case["guards"].as_array().into_iter().flatten() {
            let row = row.as_array().unwrap();
            compared += row.len();
            accepted += row.iter().filter(|guard| guard.is_null()).count();
            // An unrelated commit since the base changes nothing.
            assert_eq!(row[0], row[1], "case={}", case["operations"]);
        }
    }
    assert!(compared > 1500, "{compared} guard outcomes");
    assert!(accepted > 50, "{accepted} accepted guards");
}
