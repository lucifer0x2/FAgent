//! Structural-sharing assertions, independent of timing and allocator estimates.
use super::{ReadScope, ReadView};
use crate::Snapshot;
use serde_json::json;
use std::sync::Arc;

fn view() -> ReadView {
    let snapshot: Snapshot = serde_json::from_value(json!({
        "schemaVersion":4,"projectId":"p","slug":"p","name":"p","revision":1,
        "nodes":[
            {"id":"p","parentId":null,"sortKey":0,"kind":"page","props":{}},
            {"id":"f","parentId":"p","sortKey":0,"kind":"frame","props":{}},
            {"id":"t","parentId":"f","sortKey":0,"kind":"text","props":{"text":"before"}}
        ],
        "tokens":[],"styles":[],"assets":[],"fonts":[],"notes":[],
        "retention":{"oldestCommitRevision":0,"idempotencyPolicy":"bounded"}
    }))
    .unwrap();
    ReadView::new(snapshot)
}

#[test]
fn plain_property_edit_reuses_indexes_and_untouched_payloads() {
    let before = view();
    let mut text = before.node("t").unwrap().clone();
    text.props["text"] = json!("after");
    let after = before.advance(2, [("t", Some(&text))], [], None);
    assert!(
        Arc::ptr_eq(&before.index, &after.index),
        "plain properties rebuild no indexes"
    );
    assert!(Arc::ptr_eq(
        before.nodes.get("f").unwrap(),
        after.nodes.get("f").unwrap()
    ));
    assert!(!Arc::ptr_eq(
        before.nodes.get("t").unwrap(),
        after.nodes.get("t").unwrap()
    ));
    assert_eq!(before.scoped().node("t").unwrap().props["text"], "before");
    assert_eq!(after.scoped().node("t").unwrap().props["text"], "after");
}

#[test]
fn nested_generic_references_do_not_become_master_or_navigation_edges() {
    let before = view();
    let mut text = before.node("t").unwrap().clone();
    text.props["metadata"] = json!({"componentId":"missing","to":"f"});
    let after = before.advance(2, [("t", Some(&text))], [], None);
    assert_eq!(
        after
            .reference_users("component:missing")
            .collect::<Vec<_>>(),
        ["t"]
    );
    assert!(after.index.masters.get("t").is_none());
    assert!(after.index.incoming.get("f").is_none());
    let scoped = after.scoped();
    let expanded = scoped.expand(ReadScope::Target("f"), true).unwrap();
    assert_eq!(expanded.dependencies.len(), 3);
    assert!(scoped.navigation_nodes(&[], "f").is_empty());
}

#[test]
fn resource_delta_keeps_old_resources_and_updates_only_its_edges() {
    let before = view();
    let mut meta = before.metadata().as_ref().clone();
    meta.tokens.push(
        serde_json::from_value(
            json!({"id":"t","name":"t","type":"color","value":{"aliasId":"base"}}),
        )
        .unwrap(),
    );
    let token_ids = vec!["t".into()];
    let after = before.advance(2, [], [], Some((meta, &token_ids, &[])));
    assert!(before.metadata().tokens.is_empty());
    assert_eq!(after.metadata().tokens.len(), 1);
    assert_eq!(
        after
            .index
            .dependencies
            .get("token:t")
            .unwrap()
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        ["token:base"]
    );
    assert!(Arc::ptr_eq(
        before.nodes.get("t").unwrap(),
        after.nodes.get("t").unwrap()
    ));
}

#[test]
fn i5_ordered_adjacency_changes_only_successor_and_keeps_json_header() {
    let before = view();
    let mut second = before.node("t").unwrap().clone();
    second.id = "a".into();
    second.sort_key = 2;
    second.props = json!({"text":"Overview", "name":"raw", "custom":{"unknown":[null, 3, true]}});
    let before = before.advance(2, [("a", Some(&second))], [], None);
    let mut changed = second.clone();
    changed.sort_key = -1;
    let after = before.advance(3, [("a", Some(&changed))], [], None);
    assert_eq!(before.children("f").collect::<Vec<_>>(), ["t", "a"]);
    assert_eq!(after.children("f").collect::<Vec<_>>(), ["a", "t"]);
    let scoped = after.scoped();
    assert_eq!(
        scoped
            .children("f")
            .map(|n| n.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "t"]
    );
    let snapshot = after.to_snapshot();
    assert_eq!(
        snapshot.nodes.iter().find(|n| n.id == "a").unwrap().props,
        second.props
    );
    assert_eq!(snapshot.retention, before.metadata().retention);
    assert_eq!(snapshot.name, before.metadata().name);
    assert_eq!(
        after.node("a").unwrap().typed_props().text(),
        Some("Overview")
    );
    assert_eq!(after.node("a").unwrap().typed_props().name(), Some("raw"));
    let rebuilt = ReadView::new(snapshot);
    assert_eq!(
        rebuilt.children("f").collect::<Vec<_>>(),
        after.children("f").collect::<Vec<_>>()
    );
}
