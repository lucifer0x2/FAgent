//! One scenario per end-of-batch delta rule and dependency path for tokens, styles and
//! themes, each checked against the reference (`Candidate::apply` per operation + full
//! `validate()`): the batch is rejected for the error the closure must reach, with the
//! same code and field, or accepted when the batch repairs what it broke.
mod support;

use fagent_scene_input::NodeKind;
use serde_json::json;
use support::scenario::*;
use support::*;

#[test]
fn a_size_token_change_reaches_descendants() {
    let mut state = base(&[
        token("dim", "dimension", json!(100)),
        update("f1", json!({"width": {"tokenId": "dim"}}), true),
        create(
            "w",
            NodeKind::Frame,
            "f1",
            json!({"width": "fill", "height": 10,
            "layout": {"mode": "row", "wrap": true}}),
        ),
    ]);
    judge(
        &mut state,
        &[token("dim", "dimension", json!(0))],
        fails("width"),
    );
    judge(&mut state, &[token("dim", "dimension", json!(50))], ACCEPT);
}

#[test]
fn a_size_token_reached_through_an_alias_and_a_theme_branch() {
    let mut state = base(&[
        token("dim", "dimension", json!(100)),
        token("alias", "dimension", json!({"aliasId": "dim"})),
        token(
            "dark",
            "color",
            json!({"default": "#000000", "themes": {"night": "#111111"}}),
        ),
        update("f1", json!({"width": {"tokenId": "alias"}}), true),
        create(
            "w",
            NodeKind::Frame,
            "f1",
            json!({"width": "fill", "height": 10,
            "layout": {"mode": "row", "wrap": true}}),
        ),
    ]);
    let night = json!({"default": 100, "themes": {"night": 0}});
    judge(
        &mut state,
        &[token("dim", "dimension", night)],
        fails("width"),
    );
}

#[test]
fn a_theme_scope_reaches_descendants_and_instances() {
    // Every node is checked under every declared theme, so a branch invalid for a node
    // can only exist where a scope pins another theme: here `dusk` on f1 and on m1.
    let size = json!({"default": 12, "themes": {"night": 0, "dusk": 12}});
    let mut state = base(&[
        token("size", "dimension", size),
        update("f1", json!({"theme": "dusk"}), true),
        update("m1", json!({"theme": "dusk"}), true),
        update("m1t", json!({"fontSize": {"tokenId": "size"}}), true),
        update("gt", json!({"fontSize": {"tokenId": "size"}}), true),
    ]);
    // `gt` (under g, under f1) and i1's copy of m1t resolve `night` once f1 or i1
    // scopes it.
    let night = json!({"theme": "night"});
    judge(
        &mut state,
        &[update("f1", night.clone(), true)],
        fails("fontSize"),
    );
    judge(
        &mut state,
        &[update("i1", night.clone(), true)],
        fails("fontSize"),
    );
    let repaired = [
        update("f1", night, true),
        update("gt", json!({"fontSize": 12}), true),
    ];
    // i1's root carries its master's `dusk` scope, so its copy of m1t keeps `dusk`.
    judge(&mut state, &repaired, ACCEPT);
}

#[test]
fn a_theme_no_token_declares_any_more_fails_its_scopes() {
    let mut state = base(&[
        token(
            "dark",
            "color",
            json!({"default": "#000000", "themes": {"night": "#111111"}}),
        ),
        update("f2", json!({"theme": "night"}), true),
    ]);
    let plain = token("dark", "color", json!("#000000"));
    judge(
        &mut state,
        std::slice::from_ref(&plain),
        Some(("UNKNOWN_THEME", Some("props.theme"))),
    );
    judge(
        &mut state,
        &[plain, update("f2", json!({"theme": null}), true)],
        ACCEPT,
    );
}

#[test]
fn a_style_change_reaches_its_users() {
    let mut state = base(&[
        style("s", "fill", json!("#ff0000")),
        update("a", json!({"styleId": "s"}), true),
    ]);
    let text = json!({"fontSize": 12});
    judge(&mut state, &[style("s", "text", text)], fails("styleId"));
}

#[test]
fn a_token_type_change_reaches_styles_using_it() {
    let mut state = base(&[
        token("c", "color", json!("#ff0000")),
        style("s", "fill", json!({"tokenId": "c"})),
    ]);
    let ops = [token("c", "dimension", json!(4))];
    judge(&mut state, &ops, fails("tokenId"));
}
