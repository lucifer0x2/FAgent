//! Configurable node cap: a custom limit replaces the scene-input `MAX_NODES` on load
//! and on batch end (checked on the final state, no operation index), and the error
//! names the limit in force.
mod support;

use fagent_node_memory::{Limits, ProjectState};
use fagent_scene_input::read_scope::ReadScope;
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
    assert_eq!(error.op_index, None, "the cap is a batch-level rule");
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
    assert_eq!(error.op_index, None);
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

#[test]
fn cap_is_checked_on_the_final_state_of_the_batch() {
    let limits = Limits { max_nodes: 3 };
    let mut state = ProjectState::from_snapshot_with(flat(3), limits).expect("at the cap");
    let before = observe(&state);
    // Over the cap mid-batch, back under it at the end: accepted, as Candidate does.
    state
        .apply(&[
            create("x", NodeKind::Rectangle, "p", json!({})),
            delete("n1"),
        ])
        .expect("final count is within the cap");
    assert_eq!(state.len(), 3);
    // Over the cap at the end: rejected with no operation index, state untouched.
    let held = observe(&state);
    let error = state
        .apply(&[
            create("y", NodeKind::Rectangle, "p", json!({})),
            create("z", NodeKind::Rectangle, "p", json!({})),
            delete("x"),
        ])
        .expect_err("final count is over the cap");
    assert_eq!(error.field.as_deref(), Some("nodes"));
    assert_eq!(error.op_index, None);
    assert_unchanged(&state, &held);
    assert_ne!(held.revision, before.revision);
}

/// The expanded-node limit follows the custom cap too: two instances of a master with
/// `MAX_NODES / 2` descendants expand past `MAX_NODES` but fit twice that cap.
#[test]
fn higher_limit_expands_instances_beyond_max_nodes() {
    let half = MAX_NODES / 2;
    let mut nodes = vec![
        json!({"id":"p","kind":"page","parentId":null,"sortKey":0,"props":{}}),
        json!({"id":"f","kind":"frame","parentId":"p","sortKey":0,"props":{"width":10,"height":10}}),
        json!({"id":"lib","kind":"page","parentId":null,"sortKey":1,"props":{}}),
        json!({"id":"m","kind":"frame","parentId":"lib","sortKey":0,"props":{"width":10,"height":10,"component":{"name":"M"}}}),
    ];
    for i in 0..half {
        nodes.push(
            json!({"id":format!("r{i}"),"kind":"rectangle","parentId":"m","sortKey":i,"props":{}}),
        );
    }
    let ops = [
        create("i1", NodeKind::Instance, "f", json!({"componentId":"m"})),
        create("i2", NodeKind::Instance, "f", json!({"componentId":"m"})),
    ];
    let mut state = ProjectState::from_snapshot(snapshot(nodes.clone())).expect("loads");
    let error = state.apply(&ops).expect_err("over the default cap");
    assert_eq!(error.code, "INSTANCE_LIMIT", "{error:?}");
    let limits = Limits {
        max_nodes: 2 * MAX_NODES,
    };
    let mut state = ProjectState::from_snapshot_with(snapshot(nodes), limits).expect("loads");
    state.apply(&ops).expect("fits the custom cap");
    // Reads expand under the same cap as writes.
    let view = state.view();
    let (expanded, _) = view
        .scoped()
        .expand_nodes(ReadScope::Target("p"))
        .expect("the page reads under the custom cap");
    assert_eq!(expanded.len(), 2 + 2 * (half + 1));
}
