//! Loading and every read on the small hand-written project.
mod support;

use fagent_node_memory::ProjectState;
use serde_json::json;
use support::*;

fn ids<'a>(nodes: impl Iterator<Item = &'a fagent_scene_input::Node>) -> Vec<&'a str> {
    nodes.map(|n| n.id.as_str()).collect()
}

#[test]
fn children_follow_sort_key_then_id() {
    let state = load(small());
    assert_eq!(ids(state.pages()), ["lib", "p1", "p2"]);
    assert_eq!(kids(&state, "f1"), ["b", "a", "c", "g"]);
    assert_eq!(kids(&state, "p2"), ["f2", "t2"]);
    assert_eq!(state.children("a").unwrap().len(), 0);
    assert!(state.children("missing").is_none());
    assert_eq!(state.len(), 18);
    assert_eq!(state.revision(), 1);
    assert_eq!(state.project_id(), "proj");
}

#[test]
fn subtree_is_preorder_and_borrowed() {
    let state = load(small());
    let walk = ids(state.subtree("p1").unwrap());
    assert_eq!(walk, ["p1", "f1", "b", "a", "c", "g", "gt", "i1"]);
    assert_eq!(ids(state.subtree("gt").unwrap()), ["gt"]);
    let first = state.subtree("f1").unwrap().next().unwrap();
    assert!(std::ptr::eq(first, state.node("f1").unwrap()));
    assert!(state.subtree("missing").is_none());
}

#[test]
fn page_root_frame_and_master_lookups() {
    let state = load(small());
    let id = |n: Option<&fagent_scene_input::Node>| n.map(|n| n.id.clone());
    assert_eq!(id(state.page_of("gt")), Some("p1".into()));
    assert_eq!(id(state.page_of("p1")), Some("p1".into()));
    assert_eq!(id(state.root_frame_of("gt")), Some("f1".into()));
    assert_eq!(id(state.root_frame_of("f1")), Some("f1".into()));
    assert_eq!(id(state.root_frame_of("t2")), Some("t2".into()));
    assert_eq!(id(state.root_frame_of("p1")), None);
    assert_eq!(id(state.master_of("m1nt")), Some("m1".into()));
    assert_eq!(id(state.master_of("m1n")), Some("m1".into()));
    assert_eq!(id(state.master_of("m1")), Some("m1".into()));
    assert_eq!(id(state.master_of("m2")), Some("m2".into()));
    assert_eq!(id(state.master_of("gt")), None);
    assert_eq!(id(state.master_of("missing")), None);
}

#[test]
fn instances_of_components() {
    let state = load(small());
    assert_eq!(ids(state.instances_of("m1")), ["i1"]);
    assert_eq!(ids(state.instances_of("m2")), ["i2"]);
    assert_eq!(state.instances_of("m1n").count(), 0);
    assert_eq!(state.instances_of("missing").count(), 0);
}

#[test]
fn snapshot_round_trips() {
    let state = load(small());
    let again = load(state.to_snapshot());
    assert!(again.to_snapshot() == state.to_snapshot());
    assert_eq!(again.index_dump(), state.index_dump());
    assert!(state.estimated_bytes() > 0);
}

#[test]
fn load_rejects_structural_errors() {
    let node = |id: &str, kind: &str, parent: Option<&str>, props: serde_json::Value| json!({"id":id,"kind":kind,"parentId":parent,"sortKey":0,"props":props});
    let page = node("p", "page", None, json!({}));
    let cases = [
        (
            vec![page.clone(), node("p", "frame", Some("p"), json!({}))],
            "VALIDATION_FAILED",
            "id",
        ),
        (
            vec![node("p", "page", Some("x"), json!({}))],
            "VALIDATION_FAILED",
            "parentId",
        ),
        (
            vec![page.clone(), node("f", "frame", None, json!({}))],
            "VALIDATION_FAILED",
            "parentId",
        ),
        (
            vec![page.clone(), node("f", "frame", Some("x"), json!({}))],
            "VALIDATION_FAILED",
            "parentId",
        ),
        (
            vec![page.clone(), node("a/b", "frame", Some("p"), json!({}))],
            "INSTANCE_CHILD_READONLY",
            "",
        ),
        (
            vec![page.clone(), node("f", "frame", Some("p"), json!(1))],
            "VALIDATION_FAILED",
            "props",
        ),
        (
            vec![
                page.clone(),
                node("a", "frame", Some("b"), json!({})),
                node("b", "frame", Some("a"), json!({})),
            ],
            "VALIDATION_FAILED",
            "parentId",
        ),
        (
            vec![
                page,
                node("t", "text", Some("p"), json!({})),
                node("x", "rectangle", Some("t"), json!({})),
            ],
            "VALIDATION_FAILED",
            "parentId",
        ),
    ];
    for (i, (nodes, code, field)) in cases.into_iter().enumerate() {
        let error = ProjectState::from_snapshot(snapshot(nodes))
            .err()
            .expect("rejected");
        assert_eq!(error.code, code, "case {i}");
        assert_eq!(error.field.as_deref().unwrap_or(""), field, "case {i}");
    }
}
