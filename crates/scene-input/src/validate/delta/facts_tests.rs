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
    let (after, next) = judged(before, facts, ops, MAX_NODES);
    (after, next.unwrap())
}

/// Applies `ops` to `before` and runs the delta rules under `max_nodes`.
fn judged(
    before: &Candidate,
    facts: &Facts,
    ops: &[Value],
    max_nodes: usize,
) -> (Candidate, crate::Result<Facts>) {
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
        max_nodes,
    };
    let next = validate_batch(before, &after, &batch, facts);
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

/// A stored project already over the expanded limit accepts a batch that leaves the
/// excess alone and rejects one that grows it.
#[test]
fn a_stored_excess_blocks_only_batches_that_grow_it() {
    let c = candidate();
    let facts = of(&c);
    let limit = facts.total() - 1;
    let rename = [json!({"op":"update_node","id":"f","props":{"name":"F"},"merge":true})];
    let (_, next) = judged(&c, &facts, &rename, limit);
    assert_eq!(next.expect("neutral edit").total(), facts.total());
    let grow = [
        json!({"op":"create_node","id":"ia2","kind":"instance","parentId":"f","props":{"componentId":"a"}}),
    ];
    let (_, next) = judged(&c, &facts, &grow, limit);
    assert_eq!(next.err().expect("excess grows").code, "INSTANCE_LIMIT");
}

/// Stored masters nested far past the component depth limit, each holding two
/// instances of the next: counting stays bounded instead of overflowing.
#[test]
fn an_invalid_deep_master_chain_counts_without_overflow() {
    let node = |id: String, kind: &str, parent: &str, props: Value| json!({"id":id,"kind":kind,"parentId":parent,"sortKey":0,"props":props});
    let mut nodes = vec![json!({"id":"lib","kind":"page","parentId":null,"sortKey":0,"props":{}})];
    for i in 0..70 {
        let props = json!({"width":10,"height":10,"component":{"name":format!("M{i}")}});
        nodes.push(node(format!("m{i}"), "frame", "lib", props));
        for copy in 0..2 {
            let props = json!({"componentId":format!("m{}", i + 1)});
            nodes.push(node(
                format!("m{i}i{copy}"),
                "instance",
                &format!("m{i}"),
                props,
            ));
        }
    }
    let snapshot: Snapshot = serde_json::from_value(json!({
        "schemaVersion":4,"projectId":"p","slug":"p","name":"p","revision":1,
        "retention":{"oldestCommitRevision":0,"idempotencyPolicy":"bounded"},
        "tokens":[],"styles":[],"assets":[],"fonts":[],"notes":[],"nodes":nodes
    }))
    .unwrap();
    let c = Candidate::from_snapshot(&snapshot);
    let total = of(&c).total();
    assert!(total > c.nodes.len() && total < 1 << 20, "{total}");
}

/// Five instances of a three-rectangle master hold a stored total of 26 over a limit of
/// 12. Deleting a master rectangle shrinks the total to 20 although the five instances
/// still expand to 15 nodes, past the limit, so it is accepted; growing is not.
#[test]
fn a_repair_that_shrinks_a_stored_excess_is_accepted() {
    let node = |id: &str, kind: &str, parent: Option<&str>, props: Value| json!({"id":id,"kind":kind,"parentId":parent,"sortKey":0,"props":props});
    let mut nodes = vec![
        node("p", "page", None, json!({})),
        node("f", "frame", Some("p"), json!({"width":10,"height":10})),
        node(
            "m",
            "frame",
            Some("p"),
            json!({"width":10,"height":10,"component":{"name":"M"}}),
        ),
    ];
    for i in 0..3 {
        nodes.push(node(&format!("r{i}"), "rectangle", Some("m"), json!({})));
    }
    for i in 0..5 {
        nodes.push(node(
            &format!("i{i}"),
            "instance",
            Some("f"),
            json!({"componentId":"m"}),
        ));
    }
    let snapshot: Snapshot = serde_json::from_value(json!({
        "schemaVersion":4,"projectId":"p","slug":"p","name":"p","revision":1,
        "retention":{"oldestCommitRevision":0,"idempotencyPolicy":"bounded"},
        "tokens":[],"styles":[],"assets":[],"fonts":[],"notes":[],"nodes":nodes
    }))
    .unwrap();
    let c = Candidate::from_snapshot(&snapshot);
    let facts = of(&c);
    assert_eq!(facts.total(), 26);
    let shrink = [json!({"op":"delete_subtree","id":"r0"})];
    let (after, next) = judged(&c, &facts, &shrink, 12);
    assert_eq!(next.expect("the excess shrinks").total(), expanded(&after));
    let grow = [json!({"op":"create_node","id":"r3","kind":"rectangle","parentId":"m","props":{}})];
    let error = judged(&c, &facts, &grow, 12)
        .1
        .err()
        .expect("the excess grows");
    assert_eq!(
        (error.code.as_str(), error.message.as_str()),
        ("INSTANCE_LIMIT", "Expanded project exceeds 12 nodes")
    );
}
