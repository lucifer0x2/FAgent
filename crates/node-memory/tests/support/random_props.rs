//! Operations whose outcome property validation decides: bad value types and ranges,
//! unknown keys, references to missing tokens, styles and fonts, malformed styles,
//! token alias chains (with cycles and type mismatches) and non-object props. A share
//! is valid so accepted batches keep moving the project.
use super::random_ops::{Gen, Pool};
use super::text;
use fagent_node_memory::Operation;
use fagent_scene_input::{NodeKind, Style, Token};
use serde_json::{json, Value};

/// Rectangle props: valid ones first, then each kind of property error.
fn rect_props(choice: usize) -> Value {
    match choice {
        0 => json!({"width": 20, "height": 30}),
        1 => json!({"fill": "#ff0000", "opacity": 0.5}),
        2 => json!({"fill": {"tokenId": "cyc0"}}),
        3 => json!({"styleId": "bst0"}),
        4 => json!({"width": "wide"}),
        5 => json!({"width": -5}),
        6 => json!({"height": true}),
        7 => json!({"opacity": 2}),
        8 => json!({"fill": "red"}),
        9 => json!({"bogus": 1}),
        10 => json!({"radius": "x"}),
        11 => json!({"stroke": {"width": 1}}),
        12 => json!({"links": "nope"}),
        13 => json!({"fill": {"tokenId": "nope"}}),
        14 => json!({"styleId": "nostyle"}),
        15 => json!({"name": 5}),
        _ => json!({"width": "fill"}),
    }
}

impl Gen<'_> {
    pub(super) fn invalid(&mut self, pool: &mut Pool) -> Operation {
        match self.rng.below(10) {
            0 | 1 => self.bad_create(pool),
            2 | 3 => {
                let id = self.pick_kind(pool, &[NodeKind::Rectangle, NodeKind::Frame]);
                Operation::UpdateNode {
                    id,
                    props: rect_props(self.rng.below(17)),
                    merge: self.rng.chance(50),
                }
            }
            4 => self.bad_text_update(pool),
            5 | 6 => self.chain_token(),
            7 => self.bad_token(),
            8 => self.bad_style(),
            _ => self.odd_props(pool),
        }
    }

    fn bad_create(&mut self, pool: &mut Pool) -> Operation {
        self.counter += 1;
        let id = format!("bp{}", self.counter);
        let parent = self.pick_kind(pool, &[NodeKind::Frame, NodeKind::Group]);
        let (kind, props) = match self.rng.below(4) {
            0 => (NodeKind::Text, json!({"text": "x", "fontSize": 12})),
            1 => (NodeKind::Text, json!({"text": "x", "fontId": "nofont"})),
            2 => {
                let master = self.pick(pool);
                (
                    NodeKind::Instance,
                    json!({"componentId": master, "overrides": {"bogus": {}}}),
                )
            }
            _ => (NodeKind::Rectangle, rect_props(self.rng.below(17))),
        };
        pool.nodes.push((id.clone(), kind.clone()));
        Operation::CreateNode {
            id,
            kind,
            parent_id: Some(parent),
            props,
            index: None,
        }
    }

    fn bad_text_update(&mut self, pool: &Pool) -> Operation {
        let id = self.pick_kind(pool, &[NodeKind::Text]);
        let props = match self.rng.below(5) {
            0 => text("ok"),
            1 => json!({"fontSize": "big"}),
            2 => json!({"fontId": null}),
            3 => json!({"textAlign": "middle"}),
            _ => json!({"lineHeight": {"tokenId": "cyc1"}}),
        };
        Operation::UpdateNode {
            id,
            props,
            merge: true,
        }
    }

    /// Tokens `cyc0..cyc2` that alias each other, so chains, cycles and missing
    /// targets come and go across batches.
    fn chain_token(&mut self) -> Operation {
        let id = format!("cyc{}", self.rng.below(3));
        let value = match self.rng.below(5) {
            0 | 1 => json!("#112233"),
            _ => json!({"aliasId": format!("cyc{}", self.rng.below(3))}),
        };
        let token_type = match self.rng.chance(90) {
            true => "color",
            false => "dimension",
        };
        Operation::UpsertToken {
            token: Token {
                id,
                name: "Chain".into(),
                token_type: token_type.into(),
                value,
            },
        }
    }

    fn bad_token(&mut self) -> Operation {
        let (token_type, value, name) = match self.rng.below(5) {
            0 => ("string", json!("x"), "T".to_owned()),
            1 => ("color", json!(12), "T".to_owned()),
            2 => ("dimension", json!("#fff"), "T".to_owned()),
            3 => ("color", json!("#123456"), "n".repeat(300)),
            _ => (
                "number",
                json!({"default": 1, "themes": "x"}),
                "T".to_owned(),
            ),
        };
        Operation::UpsertToken {
            token: Token {
                id: format!("btk{}", self.rng.below(2)),
                name,
                token_type: token_type.into(),
                value,
            },
        }
    }

    fn bad_style(&mut self) -> Operation {
        let (kind, value) = match self.rng.below(8) {
            0 => ("fill", json!("#00ff00")),
            1 => ("fill", json!("green")),
            2 => ("fill", json!({"styleId": "bst1"})),
            3 => ("text", json!({"fontId": "nofont"})),
            4 => ("text", json!({"fontId": "font", "fontFamily": "Noto Sans"})),
            5 => ("text", json!({"fontSize": 0})),
            6 => ("text", json!({"bogus": 1})),
            _ => ("effect", json!({})),
        };
        Operation::UpsertStyle {
            style: Style {
                id: format!("bst{}", self.rng.below(2)),
                name: "Bad".into(),
                kind: kind.into(),
                value,
            },
        }
    }

    /// Non-object props on create and update, and unknown themes.
    fn odd_props(&mut self, pool: &mut Pool) -> Operation {
        match self.rng.below(3) {
            0 => {
                self.counter += 1;
                let id = format!("bp{}", self.counter);
                Operation::CreateNode {
                    id,
                    kind: NodeKind::Rectangle,
                    parent_id: Some(self.pick_kind(pool, &[NodeKind::Frame])),
                    props: json!(5),
                    index: None,
                }
            }
            1 => Operation::UpdateNode {
                id: self.pick(pool),
                props: json!([1]),
                merge: false,
            },
            _ => Operation::UpdateNode {
                id: self.pick_kind(pool, &[NodeKind::Frame, NodeKind::Group]),
                props: json!({"theme": "nowhere"}),
                merge: true,
            },
        }
    }
}
