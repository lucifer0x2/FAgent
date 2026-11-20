//! Regression for publication cost: committing a batch publishes the staged version
//! itself, so the cost follows the rows the batch writes, never a per-row rebuild of
//! reader indexes. Before the single persistent state, renumbering 14k siblings took
//! ~9 s and deleting a 328-node root frame ~210 ms (release) in the view publish alone.
mod support;

use fagent_node_memory::{ManualClock, ProjectHandle, Registry, RegistryConfig};
use fagent_scene_input::NodeKind;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use support::*;

/// A page with a wide root frame (`wide`, `children` rectangles) and a root frame
/// `deep` holding `groups` groups of 40 rectangles each, plus an untouched frame.
fn handle(children: usize, groups: usize) -> Arc<ProjectHandle> {
    let node = |id: String, kind: &str, parent: &str, sort: usize, props: Value| json!({"id":id,"kind":kind,"parentId":parent,"sortKey":sort,"props":props});
    let frame = json!({"width":100,"height":100});
    let mut nodes = vec![
        json!({"id":"p","kind":"page","parentId":null,"sortKey":0,"props":{}}),
        node("wide".into(), "frame", "p", 0, frame.clone()),
        node("deep".into(), "frame", "p", 1, frame.clone()),
        node("calm".into(), "frame", "p", 2, frame.clone()),
        node("calm_r".into(), "rectangle", "calm", 0, json!({})),
    ];
    for i in 0..children {
        nodes.push(node(format!("w{i}"), "rectangle", "wide", i, json!({})));
    }
    for g in 0..groups {
        nodes.push(node(format!("g{g}"), "group", "deep", g, json!({})));
        for r in 0..40 {
            nodes.push(node(
                format!("g{g}r{r}"),
                "rectangle",
                &format!("g{g}"),
                r,
                json!({}),
            ));
        }
    }
    let config = RegistryConfig {
        ttl: Duration::from_secs(60),
        budget_bytes: usize::MAX,
    };
    let registry = Registry::new(config, Arc::new(ManualClock::default()));
    registry.load(snapshot(nodes)).unwrap().0
}

fn timed(handle: &ProjectHandle, op: fagent_node_memory::Operation) -> (Duration, usize) {
    let start = Instant::now();
    let delta = handle.apply(&[op]).unwrap();
    (start.elapsed(), delta.changed_ids().len())
}

#[test]
fn renumbering_a_wide_frame_publishes_in_linear_time() {
    let handle = handle(14_000, 0);
    let before = handle.read_view();
    let (elapsed, changed) = timed(&handle, create_at("front", NodeKind::Rectangle, "wide", 0));
    assert_eq!(changed, 14_001);
    let after = handle.read_view();
    assert_eq!(after.children("wide").next(), Some("front"));
    assert_eq!(after.node("w0").unwrap().sort_key, 1);
    assert_eq!(before.node("w0").unwrap().sort_key, 0);
    // Untouched rows stay shared with the previous revision.
    assert!(std::ptr::eq(
        before.node("calm_r").unwrap(),
        after.node("calm_r").unwrap()
    ));
    // Debug builds included; the old publish needed seconds even in release.
    assert!(elapsed < Duration::from_secs(2), "took {elapsed:?}");
}

#[test]
fn deleting_a_root_frame_publishes_in_linear_time() {
    let handle = handle(0, 8);
    let before = handle.read_view();
    let (elapsed, changed) = timed(&handle, delete("deep"));
    assert_eq!(changed, 1 + 8 * 41);
    let after = handle.read_view();
    assert!(after.node("g0r0").is_none() && before.node("g0r0").is_some());
    assert_eq!(after.children("p").collect::<Vec<_>>(), ["wide", "calm"]);
    assert!(std::ptr::eq(
        before.node("calm_r").unwrap(),
        after.node("calm_r").unwrap()
    ));
    assert!(elapsed < Duration::from_millis(500), "took {elapsed:?}");
    let state = handle.read();
    let old = fagent_node_memory::ProjectState::from_snapshot(before.to_snapshot()).unwrap();
    assert_eq!(state.len(), old.len() - 329);
}

#[test]
fn a_props_edit_rewrites_only_its_own_slot() {
    let handle = handle(100, 1);
    let base = handle.read().clone();
    handle
        .apply(&[update("w5", json!({"opacity":0.5}), true)])
        .unwrap();
    let state = handle.read();
    assert_eq!(state.changed_nodes_since(&base), ["w5"]);
}
