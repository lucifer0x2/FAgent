//! Snapshot-backed views: ordering, inverse references and resource dependencies.
//! Engine-backed views are covered by `fagent-node-memory`'s publication tests.
use super::{ReadScope, ReadView};
use crate::Snapshot;
use serde_json::json;
use std::collections::BTreeSet;

fn view(extra: serde_json::Value, tokens: serde_json::Value) -> ReadView {
    let mut nodes = vec![
        json!({"id":"p","parentId":null,"sortKey":0,"kind":"page","props":{}}),
        json!({"id":"f","parentId":"p","sortKey":0,"kind":"frame","props":{}}),
        json!({"id":"t","parentId":"f","sortKey":0,"kind":"text","props":{"text":"before"}}),
    ];
    nodes.extend(extra.as_array().cloned().unwrap_or_default());
    let snapshot: Snapshot = serde_json::from_value(json!({
        "schemaVersion":4,"projectId":"p","slug":"p","name":"p","revision":1,
        "nodes":nodes,"tokens":tokens,"styles":[],"assets":[],"fonts":[],"notes":[],
        "retention":{"oldestCommitRevision":0,"idempotencyPolicy":"bounded"}
    }))
    .unwrap();
    ReadView::new(snapshot)
}

#[test]
fn nested_generic_references_do_not_become_master_or_navigation_edges() {
    let meta = json!({"text":"x","metadata":{"componentId":"missing","to":"f"}});
    let view = view(
        json!([{"id":"u","parentId":"f","sortKey":1,"kind":"text","props":meta}]),
        json!([]),
    );
    let users: Vec<_> = view.reference_users("component:missing").collect();
    assert_eq!(users, ["u"]);
    assert!(view.master("u").is_none());
    assert!(view.source.linking("f").is_empty());
    let scoped = view.scoped();
    let expanded = scoped.expand(ReadScope::Target("f"), true).unwrap();
    assert_eq!(expanded.dependencies.len(), 4);
    assert!(scoped.navigation_nodes(&[], "f").is_empty());
}

#[test]
fn token_dependencies_reach_alias_targets() {
    let token = json!({"id":"t","name":"t","type":"color","value":{"aliasId":"base"}});
    let view = view(json!([]), json!([token]));
    let keys = view.resource_dependencies("token", "t");
    assert_eq!(keys, BTreeSet::from(["token:base".to_owned()]));
}

#[test]
fn ordered_adjacency_and_snapshot_round_trip() {
    let a = json!({"text":"Overview","name":"raw","custom":{"unknown":[null,3,true]}});
    let view = view(
        json!([{"id":"a","parentId":"f","sortKey":-1,"kind":"text","props":a}]),
        json!([]),
    );
    assert_eq!(view.children("f").collect::<Vec<_>>(), ["a", "t"]);
    let scoped = view.scoped();
    let children: Vec<_> = scoped.children("f").map(|n| n.id.as_str()).collect();
    assert_eq!(children, ["a", "t"]);
    assert_eq!(view.page("a"), Some("p"));
    let snapshot = view.to_snapshot();
    assert_eq!(
        snapshot.nodes.iter().find(|n| n.id == "a").unwrap().props,
        a
    );
    assert_eq!(view.node("a").unwrap().typed_props().name(), Some("raw"));
    let rebuilt = ReadView::new(snapshot);
    assert_eq!(
        rebuilt.children("f").collect::<Vec<_>>(),
        view.children("f").collect::<Vec<_>>()
    );
}
