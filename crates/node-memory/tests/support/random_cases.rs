//! Operations aimed at the end-of-batch delta rules: instances with overrides (valid and
//! not), animations with hover triggers, dimension and color tokens with theme
//! branches and aliases used in sizes, percentages and wrapping rows, theme-scoped
//! instances, and edits inside masters (props, deletes, nested instances, unmarking).
//! The scripted batches are in `random_scripted`.
use super::random_ops::{Gen, Pool};
use super::text;
use fagent_node_memory::Operation;
use fagent_scene_input::{NodeKind, Token};
use serde_json::{json, Value};

pub(super) const MASTERS: [&str; 3] = ["m1", "m1n", "m2"];

impl Gen<'_> {
    pub(super) fn case(&mut self, pool: &mut Pool) -> Operation {
        match self.rng.below(100) {
            0..=13 => self.instance_with_overrides(pool),
            14..=21 => self.override_update(pool),
            22..=35 => self.animation(pool),
            36..=49 => self.sized_token(),
            50..=63 => self.sized_node(pool),
            64..=71 => self.scoped(pool),
            72..=87 => self.master_edit(pool),
            _ => self.wrap_row(pool),
        }
    }

    /// A node of one of `kinds` (deep chain nodes rarely), a virtual id or a missing one.
    pub(super) fn pick_of(&mut self, pool: &Pool, kinds: &[NodeKind]) -> String {
        let fits = |(id, kind): &&(String, NodeKind)| kinds.contains(kind) && !id.starts_with("dz");
        let ids: Vec<&String> = pool.nodes.iter().filter(fits).map(|(id, _)| id).collect();
        match (ids.is_empty(), self.rng.below(100)) {
            (true, _) | (_, 0) => "missing".into(),
            (_, 1) => "i1/m1t".into(),
            _ => ids[self.rng.below(ids.len())].clone(),
        }
    }

    fn master(&mut self, pool: &Pool) -> String {
        match self.rng.chance(80) {
            true => MASTERS[self.rng.below(3)].into(),
            false => self.pick_of(pool, &[NodeKind::Frame]),
        }
    }

    /// A descendant of `master` in the current revision, or a near miss.
    fn descendant(&mut self, pool: &Pool, master: &str) -> String {
        let inside: Vec<&String> = pool
            .parents
            .keys()
            .filter(|id| self.ancestors(pool, id).iter().any(|a| a == master))
            .collect();
        match (inside.is_empty(), self.rng.below(10)) {
            (false, 0..=7) => inside[self.rng.below(inside.len())].clone(),
            (_, 8) => master.into(),
            _ => "componentid".into(),
        }
    }

    fn ancestors(&self, pool: &Pool, id: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut current = pool.parents.get(id);
        while let Some(parent) = current.filter(|_| out.len() < 200) {
            out.push(parent.clone());
            current = pool.parents.get(parent);
        }
        out
    }

    fn patch(&mut self) -> Value {
        match self.rng.below(10) {
            0 => json!({"text": "changed"}),
            1 => json!({"fill": "#ff0000"}),
            2 => json!({"fill": {"tokenId": self.name_of("col", 3)}}),
            3 => json!({"fill": {"tokenId": self.name_of("dim", 3)}}),
            4 => json!({"opacity": 2}),
            5 => json!({"bogus": 1}),
            6 => json!({"visible": false}),
            7 => json!({"radius": {"tokenId": self.name_of("dim", 3)}}),
            8 => json!("not an object"),
            _ => json!({"name": "renamed"}),
        }
    }

    fn name_of(&mut self, prefix: &str, count: usize) -> String {
        format!("{prefix}{}", self.rng.below(count))
    }

    fn instance_with_overrides(&mut self, pool: &mut Pool) -> Operation {
        let master = self.master(pool);
        let target = self.descendant(pool, &master);
        let mut props = json!({"componentId": master, "overrides": {target: self.patch()}});
        if self.rng.chance(25) {
            props["theme"] = json!(["night", "dusk"][self.rng.below(2)]);
        }
        if self.rng.chance(15) {
            props["width"] = json!({"tokenId": self.name_of("dim", 3)});
        }
        self.counter += 1;
        let id = format!("ci{}", self.counter);
        let parent = self.pick_of(pool, &[NodeKind::Frame, NodeKind::Group]);
        pool.nodes.push((id.clone(), NodeKind::Instance));
        Operation::CreateNode {
            id,
            kind: NodeKind::Instance,
            parent_id: Some(parent),
            props,
            index: None,
        }
    }

    fn override_update(&mut self, pool: &Pool) -> Operation {
        let id = self.pick_of(pool, &[NodeKind::Instance]);
        let master = self.master(pool);
        let target = self.descendant(pool, &master);
        let props = match self.rng.below(3) {
            0 => json!({"overrides": null}),
            _ => json!({"overrides": {target: self.patch()}}),
        };
        Operation::UpdateNode {
            id,
            props,
            merge: true,
        }
    }

    /// A hover animation on a node, triggered by an ancestor, itself or another node.
    pub(super) fn animation(&mut self, pool: &Pool) -> Operation {
        let kinds = [
            NodeKind::Rectangle,
            NodeKind::Text,
            NodeKind::Frame,
            NodeKind::Group,
        ];
        let id = self.pick_of(pool, &kinds);
        let ancestors = self.ancestors(pool, &id);
        let trigger = match (ancestors.is_empty(), self.rng.below(10)) {
            (false, 0..=5) => ancestors[self.rng.below(ancestors.len().min(3))].clone(),
            (_, 6) => id.clone(),
            (_, 7) => "missing".into(),
            _ => self.pick(pool),
        };
        let props = match self.rng.chance(15) {
            true => json!({"animations": null}),
            false => json!({"animations": [{"name": "fade", "on": "hover",
                "triggerNodeId": trigger, "duration": 200, "keyframes": [{"opacity": 0.5}]}]}),
        };
        Operation::UpdateNode {
            id,
            props,
            merge: true,
        }
    }

    /// `dim0..2` (dimension) and `col0..2` (color) tokens with literal, alias and theme
    /// branches; sometimes a type change.
    fn sized_token(&mut self) -> Operation {
        let dimension = self.rng.chance(60);
        let prefix = if dimension { "dim" } else { "col" };
        let id = self.name_of(prefix, 3);
        let mut literal = |rng: &mut super::Rng| match dimension {
            true => json!([10, 0, 50, 300, 200_000][rng.below(5)]),
            false => json!(format!("#00{:02x}00", rng.below(200))),
        };
        let branch = |gen: &mut Self, literal: &mut dyn FnMut(&mut super::Rng) -> Value| match gen
            .rng
            .chance(85)
        {
            true => literal(gen.rng),
            false => json!({"aliasId": format!("{prefix}{}", gen.rng.below(3))}),
        };
        let value = match self.rng.below(3) {
            0 => branch(self, &mut literal),
            _ => {
                let theme = ["night", "dusk"][self.rng.below(2)];
                json!({"default": branch(self, &mut literal), "themes": {theme: branch(self, &mut literal)}})
            }
        };
        let token_type = match (dimension, self.rng.chance(8)) {
            (true, false) | (false, true) => "dimension",
            _ => "color",
        };
        Operation::UpsertToken {
            token: Token {
                id,
                name: "Sized".into(),
                token_type: token_type.into(),
                value,
            },
        }
    }

    /// Sizes from tokens, percentages and fill on frames and rectangles, or a child
    /// created with them.
    fn sized_node(&mut self, pool: &mut Pool) -> Operation {
        let key = ["width", "height"][self.rng.below(2)];
        let value = match self.rng.below(6) {
            0 | 1 => json!({"tokenId": self.name_of("dim", 3)}),
            2 => json!("50%"),
            3 => json!("fill"),
            4 => json!("hug"),
            _ => json!(120),
        };
        let mut props = json!({ key: value });
        if self.rng.chance(20) {
            props["fill"] = json!({"tokenId": self.name_of("col", 3)});
        }
        if self.rng.chance(40) {
            self.counter += 1;
            let id = format!("cs{}", self.counter);
            let parent = self.pick_of(pool, &[NodeKind::Frame, NodeKind::Group]);
            pool.nodes.push((id.clone(), NodeKind::Rectangle));
            return Operation::CreateNode {
                id,
                kind: NodeKind::Rectangle,
                parent_id: Some(parent),
                props,
                index: None,
            };
        }
        let id = self.pick_of(pool, &[NodeKind::Frame, NodeKind::Rectangle]);
        Operation::UpdateNode {
            id,
            props,
            merge: true,
        }
    }

    /// `theme` on instances, frames and groups.
    fn scoped(&mut self, pool: &Pool) -> Operation {
        let kinds = [NodeKind::Instance, NodeKind::Frame, NodeKind::Group];
        let id = self.pick_of(pool, &kinds);
        let theme = [json!("night"), json!("dusk"), Value::Null][self.rng.below(3)].clone();
        Operation::UpdateNode {
            id,
            props: json!({"theme": theme}),
            merge: true,
        }
    }

    /// Props, deletes, children and nested instances inside masters; unmarking.
    fn master_edit(&mut self, pool: &mut Pool) -> Operation {
        let master = MASTERS[self.rng.below(3)];
        let mut inside = self.descendant(pool, master);
        if inside == "componentid" {
            inside = master.into();
        }
        match self.rng.below(7) {
            0 => Operation::UpdateNode {
                id: inside,
                props: self.patch(),
                merge: true,
            },
            1 => Operation::DeleteSubtree { id: inside },
            2 | 3 => {
                self.counter += 1;
                let id = format!("cm{}", self.counter);
                let (kind, props) = match self.rng.below(3) {
                    0 => (NodeKind::Text, text("in master")),
                    1 => (
                        NodeKind::Rectangle,
                        json!({"width": {"tokenId": self.name_of("dim", 3)}}),
                    ),
                    _ => {
                        let other = MASTERS[self.rng.below(3)];
                        (NodeKind::Instance, json!({"componentId": other}))
                    }
                };
                pool.nodes.push((id.clone(), kind.clone()));
                Operation::CreateNode {
                    id,
                    kind,
                    parent_id: Some(master.into()),
                    props,
                    index: None,
                }
            }
            4 => Operation::UpdateNode {
                id: master.into(),
                props: json!({"component": null}),
                merge: true,
            },
            5 => Operation::UpdateNode {
                id: master.into(),
                props: json!({"component": {"name": "Again"}}),
                merge: true,
            },
            _ => Operation::ReparentNode {
                id: inside,
                parent_id: self.pick_of(pool, &[NodeKind::Frame, NodeKind::Group]),
                index: None,
            },
        }
    }

    /// A wrapping row, whose fill children need a positive minWidth.
    fn wrap_row(&mut self, pool: &Pool) -> Operation {
        let id = self.pick_of(pool, &[NodeKind::Frame]);
        let props = match self.rng.below(4) {
            0 => json!({"layout": {"mode": "row", "wrap": true}}),
            1 => json!({"layout": null}),
            2 => json!({"width": "fill"}),
            _ => json!({"width": "fill", "minWidth": 20}),
        };
        Operation::UpdateNode {
            id,
            props,
            merge: true,
        }
    }
}
