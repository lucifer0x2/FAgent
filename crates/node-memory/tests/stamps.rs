//! Last-writer-wins groundwork: per node, the revision that last wrote each top-level
//! props key and its placement since load, rolled back with failed batches.
mod support;

use fagent_node_memory::Written;
use fagent_scene_input::NodeKind;
use serde_json::json;
use support::*;

#[test]
fn writes_stamp_keys_placement_and_creation() {
    let mut state = load(small());
    assert_eq!(state.loaded_revision(), 1);
    assert!(state.writes("a").is_empty());
    assert_eq!(state.prop_revision("a", "name"), None);
    let base = state.estimated_bytes();

    state
        .apply(&[update("a", json!({"name":"A","w":1}), false)])
        .unwrap();
    state
        .apply(&[update("a", json!({"w":{"x":1}}), true)])
        .unwrap();
    assert_eq!(state.prop_revision("a", "name"), Some(2));
    assert_eq!(state.prop_revision("a", "w"), Some(3));
    assert_eq!(state.prop_revision("a", "fill"), None);
    assert_eq!(state.placement_revision("a"), None);

    // Reordering b renumbers a as a side effect: only b's placement is stamped.
    state.apply(&[reorder("b", 3)]).unwrap();
    assert_eq!(state.placement_revision("b"), Some(4));
    assert_eq!(state.placement_revision("a"), None);
    state.apply(&[reparent("gt", "f2")]).unwrap();
    assert_eq!(state.placement_revision("gt"), Some(5));

    state
        .apply(&[create("n", NodeKind::Text, "f2", text("n"))])
        .unwrap();
    assert_eq!(state.prop_revision("n", "anything"), Some(6));
    assert_eq!(state.placement_revision("n"), Some(6));
    assert_eq!(state.writes("n"), [(Written::Created, 6)]);
    state
        .apply(&[update("n", json!({"text":"m"}), true)])
        .unwrap();
    assert_eq!(state.prop_revision("n", "text"), Some(7));
    assert_eq!(state.prop_revision("n", "fontId"), Some(6));
    let mut writes = state.writes("a");
    writes.sort_by_key(|(_, rev)| *rev);
    assert_eq!(
        writes,
        [(Written::Prop("name"), 2), (Written::Prop("w"), 3)]
    );

    let (stamped, bytes) = state.stamp_stats();
    assert_eq!(stamped, 4);
    assert!(bytes > 0 && state.estimated_bytes() >= base + bytes);
    assert_rebuild_matches(&state);
}

#[test]
fn link_pruning_stamps_links_and_failures_restore_stamps() {
    let mut state = load(small());
    state
        .apply(&[update("a", json!({"links":[{"to":"t2"}]}), false)])
        .unwrap();
    state.apply(&[delete("t2")]).unwrap();
    assert_eq!(state.prop_revision("a", "links"), Some(3));
    let before = observe(&state);
    let failing = [
        update("a", json!({"name":"x"}), false),
        reorder("b", 0),
        create("z", NodeKind::Rectangle, "f1", json!({})),
        delete("missing"),
    ];
    state.apply(&failing).unwrap_err();
    assert_unchanged(&state, &before);
    assert_eq!(state.prop_revision("a", "name"), None);
    assert_eq!(state.placement_revision("b"), None);
}
