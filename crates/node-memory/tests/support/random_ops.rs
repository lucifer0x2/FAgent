//! Random batches of stored `Operation`s over the current project, mixing valid and
//! invalid choices: missing/virtual/taken ids, cycles, non-containers, out-of-bounds
//! indexes, bad component targets, master changes, links to deleted nodes, notes,
//! tokens and styles. Props stay valid for the node kind, so property validation does
//! not decide outcomes.
use super::{text, Rng};
use fagent_node_memory::Operation;
use fagent_scene_input::{NodeKind, Snapshot};
use serde_json::{json, Value};

pub struct Gen<'a> {
    pub rng: &'a mut Rng,
    pub counter: usize,
    /// Also draw pages, root frames, tokens, styles, themes, flows and links.
    pub extended: bool,
}

/// Ids and kinds the generator picks from, refreshed per batch.
pub(super) struct Pool {
    pub(super) nodes: Vec<(String, NodeKind)>,
    pub(super) notes: Vec<String>,
}

impl Gen<'_> {
    pub fn batch(&mut self, current: &Snapshot) -> Vec<Operation> {
        let mut pool = Pool {
            nodes: current
                .nodes
                .iter()
                .map(|n| (n.id.clone(), n.kind.clone()))
                .collect(),
            notes: current.notes.iter().map(|n| n.id.clone()).collect(),
        };
        let size = 1 + self.rng.below(4);
        (0..size).map(|_| self.op(&mut pool)).collect()
    }

    pub(super) fn pick(&mut self, pool: &Pool) -> String {
        match (pool.nodes.is_empty(), self.rng.below(100)) {
            (true, _) | (_, 0..=1) => "missing".into(),
            (_, 2) => "i1/m1t".into(),
            _ => pool.nodes[self.rng.below(pool.nodes.len())].0.clone(),
        }
    }

    /// An existing node of one of `kinds` when there is one, else a missing id.
    pub(super) fn pick_kind(&mut self, pool: &Pool, kinds: &[NodeKind]) -> String {
        for _ in 0..6 {
            let id = self.pick(pool);
            let kind = pool.nodes.iter().find(|(n, _)| *n == id).map(|(_, k)| k);
            if kind.is_none_or(|k| kinds.contains(k)) {
                return id;
            }
        }
        "missing".into()
    }

    /// A link target: an existing id or a valid missing one (never virtual).
    fn target(&mut self, pool: &Pool) -> String {
        match self.pick(pool) {
            id if id.contains('/') => "missing".into(),
            id => id,
        }
    }

    fn index(&mut self) -> Option<usize> {
        self.rng.chance(50).then(|| self.rng.below(5))
    }

    fn op(&mut self, pool: &mut Pool) -> Operation {
        if self.extended && self.rng.chance(35) {
            return self.extra(pool);
        }
        match self.rng.below(100) {
            0..=29 => self.create(pool),
            30..=47 => self.update(pool),
            48..=64 => {
                let containers = [NodeKind::Frame, NodeKind::Group, NodeKind::Page];
                let id = match self.rng.chance(90) {
                    true => self.pick_kind(pool, &containers[..2]),
                    false => self.pick(pool),
                };
                Operation::ReparentNode {
                    id,
                    parent_id: self.pick_kind(pool, &containers),
                    index: self.index(),
                }
            }
            65..=75 => Operation::ReorderNode {
                id: self.pick(pool),
                index: self.rng.below(5),
            },
            76..=85 => Operation::DeleteSubtree {
                id: self.pick(pool),
            },
            86..=94 => self.note(pool),
            _ => self.resource(),
        }
    }

    fn create(&mut self, pool: &mut Pool) -> Operation {
        self.counter += 1;
        let id = match self.rng.below(100) {
            0..=2 => self.pick(pool),
            3..=4 => format!("x/{}", self.counter),
            5 if !pool.notes.is_empty() => pool.notes[0].clone(),
            6 => "bad id".into(),
            _ => format!("r{}", self.counter),
        };
        let size = json!({"width":10,"height":10});
        let (kind, props) = match self.rng.below(8) {
            0 => (NodeKind::Frame, size),
            1 => (
                NodeKind::Frame,
                json!({"width":10,"height":10,"component":{"name":"C"}}),
            ),
            2 => (NodeKind::Group, json!({})),
            3 => (NodeKind::Text, text("t")),
            4 => (
                NodeKind::Rectangle,
                json!({"links":[{"to": self.target(pool)}]}),
            ),
            5 => {
                let master = self.pick_kind(pool, &[NodeKind::Frame]);
                (NodeKind::Instance, json!({"componentId": master}))
            }
            6 => (NodeKind::Rectangle, json!({})),
            _ => (NodeKind::Page, json!({})),
        };
        let containers = [NodeKind::Frame, NodeKind::Group, NodeKind::Page];
        let parent_id = match (kind == NodeKind::Page, self.rng.chance(8)) {
            (true, false) => None,
            (false, false) => Some(self.pick_kind(pool, &containers)),
            (_, true) => Some(self.pick(pool)),
        };
        pool.nodes.push((id.clone(), kind.clone()));
        Operation::CreateNode {
            id,
            kind,
            parent_id,
            props,
            index: self.index(),
        }
    }

    fn update(&mut self, pool: &Pool) -> Operation {
        let (id, props) = match self.rng.below(7) {
            0 => (self.pick(pool), json!({"name":"renamed"})),
            1 => (
                self.pick_kind(pool, &[NodeKind::Frame]),
                json!({"component":{"name":"C"}}),
            ),
            2 => (
                self.pick_kind(pool, &[NodeKind::Frame]),
                json!({"component":null}),
            ),
            3 => (self.pick(pool), json!({})),
            4 => {
                let master = self.pick_kind(pool, &[NodeKind::Frame]);
                let instance = self.pick_kind(pool, &[NodeKind::Instance]);
                (instance, json!({"componentId": master}))
            }
            5 => {
                let to = self.target(pool);
                let source = self.pick_kind(pool, &[NodeKind::Rectangle]);
                (source, json!({"links":[{"to": to}]}))
            }
            _ => (self.pick(pool), json!({"links":null})),
        };
        // A null value is only valid as a merge-patch removal.
        let has_null = props.as_object().unwrap().values().any(Value::is_null);
        Operation::UpdateNode {
            id,
            props,
            merge: has_null || self.rng.chance(50),
        }
    }
}
