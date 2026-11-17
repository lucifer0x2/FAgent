//! Configurable node cap: a custom limit replaces the scene-input `MAX_NODES` on load
//! and on create, and the error names the limit in force.
mod support;

use fagent_node_memory::{Limits, ProjectState};
use fagent_scene_input::{NodeKind, Snapshot, MAX_NODES};
use serde_json::json;
use support::*;

/// One page with `count - 1` rectangles under it.
fn flat(count: usize) -> Snapshot {
    let mut nodes = vec![json!({"id":"p","kind":"page","parentId":null,"sortKey":0,"props":{}})];
    for i in 1..count {
        nodes.push(
            json!({"id":format!("n{i}"),"kind":"rectangle","parentId":"p","sortKey":i,"props":{}}),
        );
    }
    snapshot(nodes)
}

#[test]
fn default_limits_match_scene_input() {
    assert_eq!(Limits::default().max_nodes, MAX_NODES);
    assert_eq!(load(flat(3)).limits(), Limits::default());
}

#[test]
fn higher_limit_loads_and_creates_beyond_max_nodes() {
    let limits = Limits {
        max_nodes: MAX_NODES + 2,
    };
    let over = flat(MAX_NODES + 1);
    assert!(ProjectState::from_snapshot(over.clone()).is_err());
    let mut state = ProjectState::from_snapshot_with(over, limits).expect("within custom cap");
    assert_eq!(state.len(), MAX_NODES + 1);
    state
        .apply(&[create("last", NodeKind::Rectangle, "p", json!({}))])
        .expect("room for one more");
    let error = state
        .apply(&[create("extra", NodeKind::Rectangle, "p", json!({}))])
        .expect_err("custom cap reached");
    assert_eq!(error.field.as_deref(), Some("nodes"));
    assert!(error.message.contains(&(MAX_NODES + 2).to_string()));
}

#[test]
fn lower_limit_rejects_load_and_create() {
    let limits = Limits { max_nodes: 3 };
    let error = ProjectState::from_snapshot_with(flat(4), limits)
        .err()
        .expect("over custom cap");
    assert_eq!(error.field.as_deref(), Some("nodes"));
    assert!(error.message.contains("exceeds 3 "), "{}", error.message);
    let mut state = ProjectState::from_snapshot_with(flat(3), limits).expect("cap is inclusive");
    let error = state
        .apply(&[create("extra", NodeKind::Rectangle, "p", json!({}))])
        .expect_err("over custom cap");
    assert_eq!(error.field.as_deref(), Some("nodes"));
    assert_eq!(state.len(), 3);
}
#[test]
fn node_cap_applies_on_load_and_create() {
    let mut nodes = vec![json!({"id":"p","kind":"page","parentId":null,"sortKey":0,"props":{}})];
    for i in 1..MAX_NODES {
        nodes.push(
            json!({"id":format!("n{i}"),"kind":"rectangle","parentId":"p","sortKey":i,"props":{}}),
        );
    }
    let mut full = snapshot(nodes);
    let mut state = ProjectState::from_snapshot(full.clone()).expect("cap is inclusive");
    let op = create("extra", NodeKind::Rectangle, "p", json!({}));
    let error = state.apply(&[op]).expect_err("over cap");
    assert_eq!(error.field.as_deref(), Some("nodes"));
    state
        .apply(&[
            delete("n1"),
            create("extra", NodeKind::Rectangle, "p", json!({})),
        ])
        .expect("room after delete");
    full.nodes.push(full.nodes[1].clone());
    full.nodes.last_mut().unwrap().id = "over".into();
    let error = ProjectState::from_snapshot(full).err().expect("over cap");
    assert_eq!(error.field.as_deref(), Some("nodes"));
}
