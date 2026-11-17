//! Tokens, styles and notes held with the nodes: upserts, note operations, the
//! delete-subtree cascade, link pruning and the round trip through `to_snapshot`.
mod support;

use fagent_node_memory::Operation;
use fagent_scene_input::{NodeKind, Style, Token};
use serde_json::json;
use support::*;

fn token(id: &str, value: &str) -> Operation {
    Operation::UpsertToken {
        token: Token {
            id: id.into(),
            name: id.into(),
            token_type: "color".into(),
            value: json!(value),
        },
    }
}

fn note(id: &str, node: &str) -> Operation {
    Operation::CreateNote {
        id: id.into(),
        node_id: node.into(),
        text: "note".into(),
        author: Some("me".into()),
    }
}

#[test]
fn tokens_and_styles_upsert_by_id() {
    let mut state = load(small());
    let style = Operation::UpsertStyle {
        style: Style {
            id: "s".into(),
            name: "S".into(),
            kind: "fill".into(),
            value: json!("#000000"),
        },
    };
    let delta = state
        .apply(&[token("t_b", "#111111"), token("t_a", "#222222"), style])
        .unwrap();
    assert_eq!(delta.tokens, ["t_a", "t_b"]);
    assert_eq!(delta.styles, ["s"]);
    state.apply(&[token("t_b", "#333333")]).unwrap();
    let ids: Vec<_> = state
        .tokens()
        .map(|t| (t.id.as_str(), t.value.clone()))
        .collect();
    assert_eq!(ids, [("t_a", json!("#222222")), ("t_b", json!("#333333"))]);
    let reloaded = load(state.to_snapshot());
    assert!(reloaded.to_snapshot() == state.to_snapshot(), "round trip");
}

#[test]
fn notes_follow_their_anchor_and_cascade() {
    let mut state = load(small());
    let delta = state
        .apply(&[
            note("n1", "gt"),
            note("n2", "a"),
            Operation::ReplyNote {
                id: "n2".into(),
                text: "reply".into(),
                author: None,
            },
            Operation::UpdateNote {
                id: "n2".into(),
                text: None,
                resolved: Some(true),
                node_id: Some("i1".into()),
            },
        ])
        .unwrap();
    assert_eq!(delta.notes.upserted, ["n1", "n2"]);
    assert_eq!(state.note("n2").unwrap().replies.len(), 1);
    assert_eq!(state.notes_on("i1").count(), 1);
    assert_eq!(state.notes_on("a").count(), 0);
    // Deleting g removes the notes on gt and (moved) on i1.
    let delta = state.apply(&[delete("g")]).unwrap();
    assert_eq!(delta.notes.deleted, ["n1", "n2"]);
    assert!(delta.changed_ids().contains(&"n1".to_owned()));
    assert_eq!(state.notes().count(), 0);
    assert_rebuild_matches(&state);
}

#[test]
fn deleting_a_link_target_prunes_links() {
    let mut state = load(small());
    let links = json!({"links":[{"to":"f2"},{"to":"t2","on":"hover","action":"overlay"}]});
    state
        .apply(&[
            update("a", links, false),
            update("b", json!({"links":[{"to":"t2"}]}), false),
        ])
        .unwrap();
    assert_eq!(state.linking_to("t2").count(), 2);
    // A target deleted and recreated in the same batch keeps its links.
    let delta = state
        .apply(&[
            delete("t2"),
            create("t2", NodeKind::Text, "p2", text("again")),
        ])
        .unwrap();
    assert_eq!(delta.changed_ids(), ["t2"]);
    let delta = state.apply(&[delete("t2")]).unwrap();
    assert_eq!(delta.nodes.upserted, ["a", "b"]);
    assert_eq!(
        state.node("a").unwrap().props,
        json!({"links":[{"to":"f2"}]})
    );
    assert_eq!(state.node("b").unwrap().props, json!({}));
    assert_eq!(state.linking_to("t2").count(), 0);
    assert_eq!(state.linking_to("f2").count(), 1);
    assert_rebuild_matches(&state);
}
