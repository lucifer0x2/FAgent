//! Task28 I3 semantics: a stale `expectedRevision` commits and reports what it
//! overwrote; `strict:true` keeps the optimistic check; replay returns the report.
mod common;

use common::*;
use fagent_storage::*;
use serde_json::json;

fn update(id: &str, props: serde_json::Value) -> Operation {
    Operation::UpdateNode {
        id: id.into(),
        props,
        merge: false,
    }
}

#[test]
fn stale_bases_commit_per_property_and_strict_still_conflicts() {
    let (_temp, _store, p) = setup();
    let seed = vec![
        create("page", NodeKind::Page, None, json!({})),
        create(
            "frame",
            NodeKind::Frame,
            Some("page"),
            json!({"width":10,"height":10}),
        ),
    ];
    p.apply_batch(request(&p, 0, "seed", seed)).unwrap();
    let other = update("frame", json!({"name":"Other","width":20}));
    p.apply_batch(request(&p, 1, "other", vec![other])).unwrap();
    // Based on revision 1: overwrites `name` (written at 2), keeps `width`.
    let mine = request(&p, 1, "mine", vec![update("frame", json!({"name":"Mine"}))]);
    let result = p.apply_batch(mine.clone()).unwrap();
    assert_eq!(result.committed_revision, 3);
    let pair = ConcurrentWrite {
        node_id: "frame".into(),
        key: "name".into(),
        revision: 2,
    };
    assert_eq!(result.concurrent, Some(vec![pair]));
    assert_eq!(result.concurrent_total, Some(1));
    let frame = p
        .snapshot()
        .unwrap()
        .nodes
        .into_iter()
        .find(|n| n.id == "frame")
        .unwrap();
    assert_eq!(
        (frame.props["name"].clone(), frame.props["width"].clone()),
        (json!("Mine"), json!(20))
    );
    // Exact replay returns the stored result, report included.
    let replay = p.apply_batch(mine).unwrap();
    assert!(replay.replayed && replay.concurrent == result.concurrent);
    // A current base reports nothing; no base at all is accepted too.
    let current = p
        .apply_batch(request(
            &p,
            3,
            "current",
            vec![update("frame", json!({"x":1}))],
        ))
        .unwrap();
    assert!(current.concurrent.is_none() && !current.concurrent_unknown);
    let mut unbased = request(&p, 0, "unbased", vec![update("frame", json!({"x":2}))]);
    unbased.expected_revision = None;
    assert_eq!(p.apply_batch(unbased).unwrap().committed_revision, 5);
    // strict:true keeps today's check; a base ahead conflicts in both modes.
    let stale = strict(request(
        &p,
        1,
        "stale",
        vec![update("frame", json!({"x":3}))],
    ));
    assert_eq!(p.apply_batch(stale).unwrap_err().current_revision, Some(5));
    let ahead = request(&p, 9, "ahead", vec![update("frame", json!({"x":4}))]);
    assert_eq!(p.apply_batch(ahead).unwrap_err().code, "REVISION_CONFLICT");
    let mut blind = strict(request(
        &p,
        0,
        "blind",
        vec![update("frame", json!({"x":5}))],
    ));
    blind.expected_revision = None;
    assert_eq!(
        p.apply_batch(blind).unwrap_err().field.as_deref(),
        Some("expectedRevision")
    );
    // Invariant violations still reject, naming the node and its parent.
    let cycle = Operation::ReparentNode {
        id: "page".into(),
        parent_id: "frame".into(),
        index: None,
    };
    let error = p
        .apply_batch(request(&p, 5, "cycle", vec![cycle]))
        .unwrap_err();
    assert_eq!(
        error.field.as_deref(),
        Some("parentId"),
        "{}",
        error.message
    );
    let missing = p.apply_batch(request(
        &p,
        5,
        "gone",
        vec![update("ghost", json!({"x":1}))],
    ));
    assert!(missing.unwrap_err().message.contains("`ghost`"));
}
