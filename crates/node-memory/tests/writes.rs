//! Each node operation on success: rows, indexes and the delta.
mod support;

use fagent_scene_input::NodeKind;
use serde_json::json;
use support::*;

fn id(node: Option<&fagent_scene_input::Node>) -> Option<&str> {
    node.map(|n| n.id.as_str())
}

#[test]
fn create_appends_or_inserts_at_index() {
    let mut state = load(small());
    let delta = state
        .apply(&[
            create("n1", NodeKind::Text, "f1", text("x")),
            create_at("n0", NodeKind::Rectangle, "f1", 0),
            create("n2", NodeKind::Instance, "g", json!({"componentId":"m1n"})),
            new("p3", NodeKind::Page, None, json!({})),
            create(
                "f3",
                NodeKind::Frame,
                "p3",
                json!({"component":{"name":"M3"}}),
            ),
        ])
        .unwrap();
    assert_eq!(delta.revision, 2);
    // f1 children tie on sort keys (b 0, a 1, c 1, g 2): appending n1 renumbers them
    // 0..4, so a, c and g are rewritten; inserting n0 at 0 shifts all of them again.
    assert_eq!(
        delta.nodes.upserted,
        ["a", "b", "c", "f3", "g", "n0", "n1", "n2", "p2", "p3"]
    );
    assert!(delta.nodes.deleted.is_empty());
    assert_eq!(delta.changed_ids(), delta.nodes.upserted);
    assert_eq!(kids(&state, "f1"), ["n0", "b", "a", "c", "g", "n1"]);
    assert_eq!(state.node("n1").unwrap().sort_key, 5);
    assert_eq!(id(state.root_frame_of("n1")), Some("f1"));
    assert_eq!(id(state.root_frame_of("f3")), Some("f3"));
    assert_eq!(id(state.master_of("f3")), Some("f3"));
    assert_eq!(id(state.page_of("f3")), Some("p3"));
    assert_eq!(state.instances_of("m1n").count(), 1);
    // Pages lib 0, p1 1, p2 1 renumber to 0, 1, 2 before p3 is appended at 3.
    let pages: Vec<_> = state.pages().map(|p| (p.id.as_str(), p.sort_key)).collect();
    assert_eq!(pages, [("lib", 0), ("p1", 1), ("p2", 2), ("p3", 3)]);
    assert_rebuild_matches(&state);
}

#[test]
fn update_assigns_or_merges_and_tracks_components() {
    let mut state = load(small());
    state
        .apply(&[update("a", json!({"name":"A","w":{"x":1}}), false)])
        .unwrap();
    state
        .apply(&[update("a", json!({"w":{"y":2}}), true)])
        .unwrap();
    assert_eq!(
        state.node("a").unwrap().props,
        json!({"name":"A","w":{"x":1,"y":2}})
    );
    state
        .apply(&[update("a", json!({"w":{"z":3}}), false)])
        .unwrap();
    state
        .apply(&[update("a", json!({"name":null}), true)])
        .unwrap();
    assert_eq!(state.node("a").unwrap().props, json!({"w":{"z":3}}));
    // Making f1 a master marks its subtree; i1 still uses m1.
    let delta = state
        .apply(&[update("f1", json!({"component":{"name":"F"}}), false)])
        .unwrap();
    assert_eq!(delta.nodes.upserted, ["f1"]);
    assert_eq!(id(state.master_of("gt")), Some("f1"));
    // Retargeting an instance moves it between usage sets.
    state
        .apply(&[update("i1", json!({"componentId":"m2"}), true)])
        .unwrap();
    assert_eq!(state.instances_of("m1").count(), 0);
    assert_eq!(state.instances_of("m2").count(), 2);
    // An unused master can stop being one; its subtree loses the master.
    state
        .apply(&[update("m1", json!({"component":null}), true)])
        .unwrap();
    assert_eq!(id(state.master_of("m1t")), None);
    assert_eq!(id(state.master_of("m1nt")), Some("m1n"));
    assert_eq!(state.revision(), 8);
    assert_rebuild_matches(&state);
}

#[test]
fn reparent_moves_subtree_indexes() {
    let mut state = load(small());
    // g (with gt, i1) moves from page p1 into master m2 on the library page.
    let delta = state.apply(&[reparent("g", "m2")]).unwrap();
    assert_eq!(delta.nodes.upserted, ["g"]);
    for node in ["g", "gt", "i1"] {
        assert_eq!(id(state.page_of(node)), Some("lib"));
        assert_eq!(id(state.root_frame_of(node)), Some("m2"));
        assert_eq!(id(state.master_of(node)), Some("m2"));
    }
    // The old parent keeps its gap-free prefix and is not renumbered.
    assert_eq!(
        keyed(&state, "f1"),
        [("b".into(), 0), ("a".into(), 1), ("c".into(), 1)]
    );
    assert_eq!(state.node("g").unwrap().parent_id.as_deref(), Some("m2"));
    assert_eq!(state.node("g").unwrap().sort_key, 0);
    // A node moved directly under a page becomes its own root frame.
    state.apply(&[reparent("gt", "p2")]).unwrap();
    assert_eq!(id(state.root_frame_of("gt")), Some("gt"));
    assert_eq!(kids(&state, "p2"), ["f2", "t2", "gt"]);
    // Same-parent reparent with an index works like a reorder.
    state.apply(&[reparent_at("gt", "p2", Some(0))]).unwrap();
    assert_eq!(
        keyed(&state, "p2"),
        [("gt".into(), 0), ("f2".into(), 1), ("t2".into(), 2)]
    );
    assert_rebuild_matches(&state);
}

#[test]
fn reorder_places_at_index_among_siblings() {
    let mut state = load(small());
    let delta = state.apply(&[reorder("b", 3)]).unwrap();
    assert_eq!(kids(&state, "f1"), ["a", "c", "g", "b"]);
    assert_eq!(delta.nodes.upserted, ["a", "b"]);
    let delta = state.apply(&[reorder("b", 3)]).unwrap();
    assert_eq!(
        delta.nodes.upserted,
        ["b"],
        "a no-op reorder still writes its node"
    );
    state.apply(&[reorder("p2", 0)]).unwrap();
    assert_eq!(state.pages().next().unwrap().id, "p2");
    assert_rebuild_matches(&state);
}

#[test]
fn delete_removes_subtree_and_usage() {
    let mut state = load(small());
    let delta = state.apply(&[delete("g")]).unwrap();
    assert_eq!(delta.nodes.deleted, ["g", "gt", "i1"]);
    assert!(delta.nodes.upserted.is_empty());
    assert!(state.node("gt").is_none());
    assert_eq!(state.instances_of("m1").count(), 0);
    // With no users left, the master's whole subtree (nested master too) can go.
    let delta = state.apply(&[delete("m1")]).unwrap();
    assert_eq!(delta.nodes.deleted, ["m1", "m1n", "m1nt", "m1t"]);
    assert_eq!(state.len(), 11);
    assert_rebuild_matches(&state);
}

#[test]
fn delete_with_users_inside_and_recreate_in_one_batch() {
    let mut state = load(small());
    // i2 (using m2) moves into m1; deleting the library page then removes both masters
    // with all their users, and m2 is recreated elsewhere in the same batch.
    let delta = state
        .apply(&[
            reparent("i2", "m1"),
            delete("g"),
            delete("lib"),
            create(
                "m2",
                NodeKind::Frame,
                "p2",
                json!({"component":{"name":"M2"}}),
            ),
            create("tmp", NodeKind::Text, "f2", text("x")),
            delete("tmp"),
        ])
        .unwrap();
    assert_eq!(delta.nodes.upserted, ["m2"]);
    let deleted = ["g", "gt", "i1", "i2", "lib", "m1", "m1n", "m1nt", "m1t"];
    assert_eq!(delta.nodes.deleted, deleted);
    // `tmp` was created and deleted: no row, but a changed id (as `Candidate` says).
    assert!(delta.changed_ids().contains(&"tmp".to_owned()));
    assert_eq!(state.instances_of("m2").count(), 0);
    assert_rebuild_matches(&state);
}
