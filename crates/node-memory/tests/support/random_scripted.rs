//! Scripted batches for the delta-rule cases: each passes through or ends in a state
//! only end-of-batch rules see. Deep chains with a subtree moved past depth 128 (and
//! often back), links to frames created later in the batch, masters unmarked and marked
//! again, and batches with several end-of-batch errors.
use super::random_cases::MASTERS;
use super::random_ops::{Gen, Pool};
use fagent_node_memory::Operation;
use fagent_scene_input::NodeKind;
use serde_json::json;

impl Gen<'_> {
    /// A short batch built to pass through or end in a state only end-of-batch rules see.
    pub(super) fn scripted(&mut self, pool: &mut Pool) -> Vec<Operation> {
        match self.rng.below(6) {
            0 | 1 => self.deep_move(pool),
            2 => {
                let mut ops = self.link_then_create(pool);
                ops.push(self.op(pool));
                ops
            }
            3 | 4 => self.unmark_and_mark(pool),
            _ => (0..3).map(|_| self.several(pool)).collect(),
        }
    }

    /// A chain of frames under a page down to depth 127, a subtree with children moved
    /// under its tip (its children land past depth 128), then often the subtree moved
    /// back and the chain deleted: the batch passes through a state only end-of-batch
    /// validation would reject.
    fn deep_move(&mut self, pool: &mut Pool) -> Vec<Operation> {
        self.counter += 1;
        let page = self.pick_of(pool, &[NodeKind::Page]);
        let root = format!("dz{}_0", self.counter);
        let mut parent = page;
        let mut ops: Vec<Operation> = (0..126)
            .map(|i| {
                let id = format!("dz{}_{i}", self.counter);
                Operation::CreateNode {
                    id: id.clone(),
                    kind: NodeKind::Frame,
                    parent_id: Some(std::mem::replace(&mut parent, id)),
                    props: json!({"width": 10, "height": 10}),
                    index: None,
                }
            })
            .collect();
        let parents: Vec<&String> = pool.parents.values().collect();
        let moved = match parents.is_empty() {
            true => "missing".to_owned(),
            false => parents[self.rng.below(parents.len())].clone(),
        };
        ops.push(Operation::ReparentNode {
            id: moved.clone(),
            parent_id: parent,
            index: None,
        });
        if self.rng.chance(60) {
            if let Some(back) = pool.parents.get(&moved) {
                ops.push(Operation::ReparentNode {
                    id: moved,
                    parent_id: back.clone(),
                    index: None,
                });
            }
            ops.push(Operation::DeleteSubtree { id: root });
        }
        ops
    }

    /// A link to a root frame the same batch creates afterwards.
    fn link_then_create(&mut self, pool: &mut Pool) -> Vec<Operation> {
        self.counter += 1;
        let frame = format!("lf{}", self.counter);
        let source = self.pick_of(pool, &[NodeKind::Rectangle, NodeKind::Frame]);
        let page = self.pick_of(pool, &[NodeKind::Page]);
        pool.nodes.push((frame.clone(), NodeKind::Frame));
        vec![
            Operation::UpdateNode {
                id: source,
                props: json!({"links": [{"to": frame}]}),
                merge: true,
            },
            Operation::CreateNode {
                id: frame,
                kind: NodeKind::Frame,
                parent_id: Some(page),
                props: json!({"width": 100, "height": 100}),
                index: None,
            },
        ]
    }

    /// A master unmarked, an instance edit, and the master marked again (or not).
    fn unmark_and_mark(&mut self, pool: &mut Pool) -> Vec<Operation> {
        let master = MASTERS[self.rng.below(3)];
        let mut ops = vec![Operation::UpdateNode {
            id: master.into(),
            props: json!({"component": null}),
            merge: true,
        }];
        ops.push(self.case(pool));
        if self.rng.chance(70) {
            ops.push(Operation::UpdateNode {
                id: master.into(),
                props: json!({"component": {"name": "Back"}}),
                merge: true,
            });
        }
        ops
    }

    /// One of several end-of-batch errors: a dangling link, a duplicate flow, a bad
    /// trigger, an unmarked master.
    fn several(&mut self, pool: &mut Pool) -> Operation {
        match self.rng.below(4) {
            0 => Operation::UpdateNode {
                id: self.pick_of(pool, &[NodeKind::Rectangle]),
                props: json!({"links": [{"to": "nowhere"}]}),
                merge: true,
            },
            1 => Operation::UpdateNode {
                id: self.pick_of(pool, &[NodeKind::Frame]),
                props: json!({"flowStart": "dup"}),
                merge: true,
            },
            2 => self.animation(pool),
            _ => Operation::UpdateNode {
                id: MASTERS[self.rng.below(3)].into(),
                props: json!({"component": null}),
                merge: true,
            },
        }
    }
}
