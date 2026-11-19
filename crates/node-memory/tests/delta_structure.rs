//! One scenario per end-of-batch delta rule and dependency path for components,
//! triggers, navigation, depth and notes, checked like `delta_rules`.
mod support;

use fagent_node_memory::Operation;
use fagent_scene_input::NodeKind;
use serde_json::json;
use support::scenario::*;
use support::*;

#[test]
fn an_unmarked_master_fails_its_instances_unless_marked_again() {
    let mut state = base(&[]);
    let unmark = update("m2", json!({"component": null}), true);
    judge(
        &mut state,
        std::slice::from_ref(&unmark),
        fails("props.componentId"),
    );
    let mark = update("m2", json!({"component": {"name": "M2"}}), true);
    judge(&mut state, &[unmark, mark], ACCEPT);
}

#[test]
fn an_override_target_deleted_or_moved_out_of_its_master() {
    let mut state = base(&[update(
        "i1",
        json!({"overrides": {"m1t": {"text": "x"}}}),
        true,
    )]);
    judge(&mut state, &[delete("m1t")], fails("props.overrides.m1t"));
    judge(
        &mut state,
        &[reparent("m1t", "f1")],
        fails("props.overrides.m1t"),
    );
    let ops = [reparent("m1t", "f1"), reparent("m1t", "m1")];
    judge(&mut state, &ops, ACCEPT);
}

#[test]
fn a_nested_master_edit_reaches_outer_instances() {
    let mut state = base(&[update(
        "i1",
        json!({"overrides": {"m1nt": {"text": "x"}}}),
        true,
    )]);
    // i1 expands m1, which holds the nested master m1n and its text m1nt.
    judge(&mut state, &[delete("m1nt")], fails("props.overrides.m1nt"));
}

#[test]
fn instance_cycles_through_nested_instances() {
    let mut state = base(&[create(
        "n1",
        NodeKind::Instance,
        "m2",
        json!({"componentId": "m1"}),
    )]);
    let ops = [create(
        "n2",
        NodeKind::Instance,
        "m1",
        json!({"componentId": "m2"}),
    )];
    judge(&mut state, &ops, Some(("INSTANCE_CYCLE", None)));
}

#[test]
fn a_trigger_ancestor_moved_out_of_the_root_frame() {
    let anim = json!({"animations": [{"name": "fade", "on": "hover", "triggerNodeId": "g",
        "duration": 200, "keyframes": [{"opacity": 0.5}]}]});
    let mut state = base(&[update("gt", anim, true)]);
    // g moves under page p2 next to f2: it is no longer inside a root frame.
    judge(
        &mut state,
        &[
            create("grp", NodeKind::Group, "p2", json!({})),
            reparent("g", "grp"),
        ],
        fails("props.animations[0].triggerNodeId"),
    );
    judge(&mut state, &[reparent("g", "f2")], ACCEPT);
}

#[test]
fn a_link_target_moved_inside_another_frame() {
    let mut state = base(&[update("a", json!({"links": [{"to": "f2"}]}), true)]);
    judge(&mut state, &[reparent("f2", "f1")], fails("links[0].to"));
    let ops = [reparent("f2", "f1"), reparent("f2", "p2")];
    judge(&mut state, &ops, ACCEPT);
}

#[test]
fn a_flow_start_frame_moved_under_a_page() {
    let mut state = base(&[
        update("f1", json!({"flowStart": "main"}), true),
        create(
            "inner",
            NodeKind::Frame,
            "f2",
            json!({"width": 10, "height": 10}),
        ),
    ]);
    let ops = [
        reparent("inner", "p2"),
        update("inner", json!({"flowStart": "main"}), true),
    ];
    judge(&mut state, &ops, fails("flowStart"));
}

#[test]
fn descendants_moved_past_depth_128_and_back() {
    let mut chain = Vec::new();
    let mut parent = "p2".to_owned();
    for i in 0..126 {
        let id = format!("c{i}");
        chain.push(create(
            &id,
            NodeKind::Frame,
            &parent,
            json!({"width": 10, "height": 10}),
        ));
        parent = id;
    }
    let mut state = base(&chain);
    judge(&mut state, &[reparent("g", "c125")], fails("parentId"));
    judge(
        &mut state,
        &[reparent("g", "c125"), reparent("g", "f1")],
        ACCEPT,
    );
}

#[test]
fn too_many_notes() {
    let mut snapshot = small();
    for i in 0..5000 {
        let note = json!({"id": format!("n{i}"), "nodeId": "a", "text": "hi"});
        snapshot.notes.push(serde_json::from_value(note).unwrap());
    }
    let mut state = load(snapshot);
    let note = Operation::CreateNote {
        id: "extra".into(),
        node_id: "b".into(),
        text: "one more".into(),
        author: None,
    };
    judge(&mut state, std::slice::from_ref(&note), fails("notes"));
    judge(&mut state, &[note, delete("a")], ACCEPT);
}

#[test]
fn a_link_target_whose_page_is_recreated_as_a_group() {
    let mut state = base(&[update("a", json!({"links": [{"to": "f2"}]}), true)]);
    // f2 keeps its id, kind and parent id, but its parent is now a group under f1.
    let ops = [
        delete("p2"),
        create("p2", NodeKind::Group, "f1", json!({})),
        create(
            "f2",
            NodeKind::Frame,
            "p2",
            json!({"width": 100, "height": 100}),
        ),
    ];
    judge(&mut state, &ops, fails("links[0].to"));
}
