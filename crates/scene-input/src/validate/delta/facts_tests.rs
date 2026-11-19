//! The per-master counters against a full expansion: on load and after batches that
//! edit a nested master, retarget, create and delete instances.
use super::super::{validate_batch, Batch};
use super::{facts, Facts};
use crate::validate::Candidate;
use crate::{instances::expand, NodeKind, Operation, Snapshot, MAX_NODES};
use serde_json::{json, Value};
use std::collections::BTreeSet;

fn candidate() -> Candidate {
    let node = |id: &str, kind: &str, parent: Option<&str>, props: Value| json!({"id":id,"kind":kind,"parentId":parent,"sortKey":0,"props":props});
    let master = |name: &str| json!({"width":10,"height":10,"component":{"name":name}});
    let snapshot: Snapshot = serde_json::from_value(json!({
        "schemaVersion":4,"projectId":"p","slug":"p","name":"p","revision":1,
        "retention":{"oldestCommitRevision":0,"idempotencyPolicy":"bounded"},
        "tokens":[],"styles":[],"assets":[],"fonts":[],"notes":[],
        "nodes":[
            node("lib", "page", None, json!({})),
            node("a", "frame", Some("lib"), master("A")),
            node("a1", "rectangle", Some("a"), json!({})),
            node("a2", "group", Some("a"), json!({})),
            node("a21", "rectangle", Some("a2"), json!({})),
            node("b", "frame", Some("lib"), master("B")),
            node("b1", "rectangle", Some("b"), json!({})),
            node("ba", "instance", Some("b"), json!({"componentId":"a"})),
            node("c", "frame", Some("lib"), master("C")),
            node("cb", "instance", Some("c"), json!({"componentId":"b"})),
            node("p", "page", None, json!({})),
            node("f", "frame", Some("p"), json!({"width":100,"height":100})),
            node("ia", "instance", Some("f"), json!({"componentId":"a"})),
            node("ib", "instance", Some("f"), json!({"componentId":"b"})),
            node("ic", "instance", Some("f"), json!({"componentId":"c"})),
        ]
    }))
    .unwrap();
    Candidate::from_snapshot(&snapshot)
}

fn expanded(c: &Candidate) -> usize {
    let nodes: Vec<_> = c.nodes.values().cloned().collect();
    expand(&nodes).unwrap().len()
}

fn of(c: &Candidate) -> Facts {
    let instances = c.nodes.values().filter(|n| n.kind == NodeKind::Instance);
    facts(c, instances.map(|n| n.id.as_str()))
}

/// Applies `ops` to `before` and runs the delta rules; returns the new state and facts.
fn step(before: &Candidate, facts: &Facts, ops: &[Value]) -> (Candidate, Facts) {
    let mut after = before.clone();
    let mut changed = BTreeSet::new();
    for op in ops {
        let op: Operation = serde_json::from_value(op.clone()).unwrap();
        after.apply(&op, &mut changed).unwrap();
    }
    let nodes: Vec<String> = changed
        .into_iter()
        .filter(|id| after.nodes.contains_key(id))
        .collect();
    let deleted: Vec<String> = before
        .nodes
        .keys()
        .filter(|id| !after.nodes.contains_key(*id))
        .cloned()
        .collect();
    let batch = Batch {
        nodes: &nodes,
        deleted: &deleted,
        notes: &[],
        tokens: &[],
        styles: &[],
        max_nodes: MAX_NODES,
    };
    let next = validate_batch(before, &after, &batch, facts).unwrap();
    (after, next)
}

#[test]
fn per_master_counters_match_a_full_expansion() {
    let c = candidate();
    let mut facts = of(&c);
    assert_eq!(facts.total(), expanded(&c));
    let create = |id: &str, kind: &str, parent: &str, props: Value| json!({"op":"create_node","id":id,"kind":kind,"parentId":parent,"props":props});
    let batches = [
        // A nested master grows: every instance of a, b and c grows.
        vec![create("a3", "rectangle", "a2", json!({}))],
        // A nested instance is retargeted and a new instance of c is created.
        vec![
            json!({"op":"update_node","id":"cb","props":{"componentId":"a"},"merge":true}),
            create("ic2", "instance", "f", json!({"componentId":"c"})),
        ],
        // A master subtree and an instance are deleted.
        vec![
            json!({"op":"delete_subtree","id":"a2"}),
            json!({"op":"delete_subtree","id":"ib"}),
        ],
    ];
    let mut state = c;
    for ops in batches {
        let (next, after) = step(&state, &facts, &ops);
        assert_eq!(after.total(), expanded(&next), "{ops:?}");
        assert_eq!(after.total(), of(&next).total(), "{ops:?}");
        (state, facts) = (next, after);
    }
}
