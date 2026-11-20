//! Operations that leave the delta-validated classes: pages and root frames, tokens
//! with themes and aliases, styles and their users, `theme`, `links` to root frames and
//! `flowStart`. Mostly valid choices with a share of invalid ones.
use super::random_ops::{Gen, Pool};
use fagent_node_memory::Operation;
use fagent_scene_input::{NodeKind, Style, Token};
use serde_json::{json, Value};

impl Gen<'_> {
    pub(super) fn extra(&mut self, pool: &mut Pool) -> Operation {
        match self.rng.below(100) {
            0..=19 => self.token(),
            20..=29 => self.style(),
            30..=39 => self.token_user(pool),
            40..=49 => self.theme(pool),
            50..=61 => self.flow_start(pool),
            62..=71 => self.link_to_root(pool),
            72..=81 => self.page(pool),
            82..=91 => self.root_frame(pool),
            _ => self.root_move(pool),
        }
    }

    fn name(&mut self, prefix: &str, count: usize) -> String {
        format!("{prefix}{}", self.rng.below(count))
    }

    fn color(&mut self) -> Value {
        json!(format!("#0000{:02x}", self.rng.below(200)))
    }

    /// A token branch: a literal, an alias to another token, or (rarely) garbage.
    fn branch(&mut self) -> Value {
        match self.rng.below(10) {
            0..=5 => self.color(),
            6..=8 => json!({"aliasId": self.name("tok", 4)}),
            _ => json!({"aliasId": "bad id"}),
        }
    }

    fn token(&mut self) -> Operation {
        let id = self.name("tok", 4);
        let value = match self.rng.below(10) {
            0..=3 => self.branch(),
            4..=7 => {
                let theme = ["night", "dusk", "default"][self.rng.below(3)];
                json!({"default": self.branch(), "themes": {theme: self.branch()}})
            }
            8 => json!({"default": self.color()}),
            _ => json!({"default": self.color(), "themes": {"night": self.color()}, "x": 1}),
        };
        let token_type = match self.rng.chance(95) {
            true => "color",
            false => "dimension",
        };
        Operation::UpsertToken {
            token: Token {
                id,
                name: "Token".into(),
                token_type: token_type.into(),
                value,
            },
        }
    }

    fn style(&mut self) -> Operation {
        let kind = match self.rng.chance(92) {
            true => "fill",
            false => "bogus",
        };
        Operation::UpsertStyle {
            style: Style {
                id: self.name("sty", 3),
                name: "Style".into(),
                kind: kind.into(),
                value: self.color(),
            },
        }
    }

    /// A node using a token or a style, so resource edits have users.
    fn token_user(&mut self, pool: &Pool) -> Operation {
        let id = self.pick_kind(pool, &[NodeKind::Rectangle, NodeKind::Frame]);
        let props = match self.rng.below(3) {
            0 => json!({"fill": {"tokenId": self.name("tok", 4)}}),
            1 => json!({"styleId": self.name("sty", 3)}),
            _ => json!({"fill": null, "styleId": null}),
        };
        Operation::UpdateNode {
            id,
            props,
            merge: true,
        }
    }

    fn theme(&mut self, pool: &Pool) -> Operation {
        let id = self.pick_kind(pool, &[NodeKind::Frame, NodeKind::Group]);
        let props = match self.rng.below(4) {
            0 => json!({"theme": null}),
            1 => json!({"theme": "dusk"}),
            _ => json!({"theme": "night"}),
        };
        Operation::UpdateNode {
            id,
            props,
            merge: true,
        }
    }

    fn flow_start(&mut self, pool: &Pool) -> Operation {
        let id = self.pick_kind(pool, &[NodeKind::Frame, NodeKind::Group]);
        let props = match self.rng.below(4) {
            0 => json!({"flowStart": null}),
            _ => json!({"flowStart": self.name("flow", 3)}),
        };
        Operation::UpdateNode {
            id,
            props,
            merge: true,
        }
    }

    fn link_to_root(&mut self, pool: &Pool) -> Operation {
        let source = self.pick_kind(pool, &[NodeKind::Rectangle, NodeKind::Frame]);
        let to = self.pick_kind(pool, &[NodeKind::Frame]);
        let to = if to.contains('/') {
            "missing".into()
        } else {
            to
        };
        Operation::UpdateNode {
            id: source,
            props: json!({"links": [{"to": to}]}),
            merge: self.rng.chance(50),
        }
    }

    fn page(&mut self, pool: &mut Pool) -> Operation {
        let page = self.pick_kind(pool, &[NodeKind::Page]);
        match self.rng.below(6) {
            0 => Operation::DeleteSubtree { id: page },
            1 => Operation::ReorderNode {
                id: page,
                index: self.rng.below(4),
            },
            2 => Operation::UpdateNode {
                id: page,
                props: json!({"name": "renamed page"}),
                merge: true,
            },
            _ => {
                self.counter += 1;
                let id = format!("pg{}", self.counter);
                pool.nodes.push((id.clone(), NodeKind::Page));
                Operation::CreateNode {
                    id,
                    kind: NodeKind::Page,
                    parent_id: None,
                    props: json!({}),
                    index: self.rng.chance(40).then(|| self.rng.below(4)),
                }
            }
        }
    }

    /// A root frame: created under a page, or an existing one reordered or deleted.
    fn root_frame(&mut self, pool: &mut Pool) -> Operation {
        let page = self.pick_kind(pool, &[NodeKind::Page]);
        match self.rng.below(4) {
            0 => Operation::DeleteSubtree {
                id: self.pick_kind(pool, &[NodeKind::Frame]),
            },
            1 => Operation::ReorderNode {
                id: self.pick_kind(pool, &[NodeKind::Frame]),
                index: self.rng.below(3),
            },
            _ => {
                self.counter += 1;
                let id = format!("rf{}", self.counter);
                pool.nodes.push((id.clone(), NodeKind::Frame));
                let mut props = json!({"width": 100, "height": 100});
                if self.rng.chance(30) {
                    props["flowStart"] = json!(self.name("flow", 3));
                }
                Operation::CreateNode {
                    id,
                    kind: NodeKind::Frame,
                    parent_id: Some(page),
                    props,
                    index: self.rng.chance(40).then(|| self.rng.below(4)),
                }
            }
        }
    }

    /// Moves a frame to or from a page, which changes root frames and navigation.
    fn root_move(&mut self, pool: &Pool) -> Operation {
        let id = self.pick_kind(pool, &[NodeKind::Frame]);
        let to_page = self.rng.chance(50);
        let parent_id = match to_page {
            true => self.pick_kind(pool, &[NodeKind::Page]),
            false => self.pick_kind(pool, &[NodeKind::Frame, NodeKind::Group]),
        };
        Operation::ReparentNode {
            id,
            parent_id,
            index: self.rng.chance(50).then(|| self.rng.below(4)),
        }
    }
}
