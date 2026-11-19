//! Full `Candidate::validate` on a batch's final candidate: layout, override,
//! reference and link errors, and cascades that leave a valid project.
mod fixture_support;
use fagent_scene_input::{validate::Candidate, *};
use fixture_support::*;
use serde_json::json;
use std::collections::BTreeSet;

/// Applies `ops` as a batch does (cascades and link pruning) and validates the
/// final candidate in full.
fn validate(before: &Snapshot, ops: &[Operation]) -> Result<()> {
    let mut candidate = Candidate::from_snapshot(before);
    let mut changed = BTreeSet::new();
    for (index, op) in ops.iter().enumerate() {
        candidate.apply(op, &mut changed).map_err(|e| e.at(index))?;
    }
    candidate.prune_links(&mut changed);
    candidate.validate()
}

#[test]
fn layout_and_override_errors_name_the_failing_field() {
    let before = fixture(3, 20);
    let error = validate(&before, &[update("master", json!({"width":"hug"}))]).unwrap_err();
    assert_eq!(error.field.as_deref(), Some("width"));
    let error = validate(&before, &[update("group0", json!({"width":"hug"}))]).unwrap_err();
    assert_eq!(error.field.as_deref(), Some("width"));
    let error = validate(
        &before,
        &[update(
            "instance0",
            json!({"overrides":{"nested":{"opacity":2}}}),
        )],
    )
    .unwrap_err();
    assert_eq!(
        error.field.as_deref(),
        Some("props.overrides.nested.opacity")
    );
    validate(
        &before,
        &[update(
            "instance0",
            json!({"overrides":{"nested":{"opacity":0.5}}}),
        )],
    )
    .unwrap();
    validate(&before, &[update("label", json!({"text":"Changed"}))]).unwrap();
}

#[test]
fn missing_references_late_invalid_operations_and_links_are_rejected() {
    let before = fixture(3, 20);
    for ops in [
        vec![update("text0", json!({"fontId":"missing"}))],
        vec![update("image0", json!({"assetId":"missing"}))],
        vec![update("text0", json!({"styleId":"missing"}))],
        vec![update("text0", json!({"fontSize":{"tokenId":"missing"}}))],
        vec![
            update("group0", json!({"width":"hug"})),
            update("group1", json!({"width":"hug"})),
        ],
        vec![
            update("text0", json!({"text":"Earlier valid"})),
            update("image0", json!({"assetId":"missing"})),
        ],
        vec![update(
            "text0",
            json!({"links":[{"on":"click","action":"navigate","to":"missing"}]}),
        )],
        vec![update(
            "master",
            json!({"links":[{"on":"click","action":"navigate","to":"frame0"}]}),
        )],
        vec![
            update("group0", json!({"width":"hug"})),
            update("nested", json!({"componentId":"outer"})),
        ],
    ] {
        assert!(validate(&before, &ops).is_err(), "{ops:?}");
    }
}

#[test]
fn deleting_a_linked_frame_with_notes_prunes_links_and_cascades_notes() {
    let mut before = fixture(2, 0);
    before
        .nodes
        .iter_mut()
        .find(|n| n.id == "text1")
        .unwrap()
        .props["links"] = json!([{"on":"click","action":"navigate","to":"frame0"}]);
    before.notes.push(Note {
        id: "note".into(),
        node_id: "child0".into(),
        text: "Note".into(),
        author: None,
        resolved: false,
        replies: vec![],
    });
    Candidate::from_snapshot(&before).validate().unwrap();
    for op in [
        Operation::DeleteSubtree {
            id: "frame0".into(),
        },
        Operation::ReparentNode {
            id: "group0".into(),
            parent_id: "frame1".into(),
            index: None,
        },
    ] {
        validate(&before, &[op]).unwrap();
    }
}

/// Untrusted stored rows (bundle import): a link to a frame on a parent cycle fails
/// instead of walking the cycle forever.
#[test]
fn a_link_to_a_frame_on_a_parent_cycle_fails() {
    let mut before = fixture(1, 0);
    let frame = |id: &str, parent: &str| {
        node(
            id,
            NodeKind::Frame,
            Some(parent),
            json!({"width":10,"height":10}),
        )
    };
    before.nodes.push(frame("loop_a", "loop_b"));
    before.nodes.push(frame("loop_b", "loop_a"));
    let page = before
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::Page)
        .unwrap()
        .id
        .clone();
    before.nodes.push(node(
        "linker",
        NodeKind::Frame,
        Some(&page),
        json!({"width":10,"height":10,"links":[{"on":"click","action":"navigate","to":"loop_a"}]}),
    ));
    assert!(Candidate::from_snapshot(&before).validate().is_err());
}

/// The override-target suggestion lists the master's descendants, nested included.
#[test]
fn a_mistyped_override_target_suggests_a_master_descendant() {
    let mut before = fixture(1, 0);
    let size = json!({"width":10,"height":10});
    before.nodes.push(node(
        "outer_box",
        NodeKind::Frame,
        Some("outer"),
        size.clone(),
    ));
    before
        .nodes
        .push(node("outer_deep", NodeKind::Frame, Some("outer_box"), size));
    validate(&before, &[]).unwrap();
    let ops = [update(
        "instance0",
        json!({"overrides":{"outer_deeep":{"visible":false}}}),
    )];
    let error = validate(&before, &ops).unwrap_err();
    assert_eq!(error.field.as_deref(), Some("props.overrides.outer_deeep"));
    let suggestion = error.node.as_ref().and_then(|n| n.suggestion.as_deref());
    assert_eq!(suggestion, Some("outer_deep"), "{error:?}");
}
