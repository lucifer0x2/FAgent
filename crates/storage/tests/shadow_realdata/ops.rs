//! Batches built from a real project's node ids: renames, no-op rewrites, merges,
//! reorders, creates, deletes, reparents, notes, master edits and token rewrites.
use fagent_storage::*;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// Deterministic xorshift, so a failing run can be repeated.
pub struct Rng(pub u64);

impl Rng {
    pub fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n.max(1) as u64) as usize
    }
}

/// One batch and, for a scoped commit, its page ids.
pub struct Batch {
    pub label: &'static str,
    pub ops: Vec<Operation>,
    pub pages: Option<Vec<String>>,
}

pub const KINDS: usize = 13;

fn op(value: Value) -> Operation {
    serde_json::from_value(value).expect("operation shape")
}

/// The `step`-th batch kind (cycling) against `s`; `serial` keeps new ids unique.
pub fn batch(s: &Snapshot, step: usize, serial: usize, rng: &mut Rng) -> Option<Batch> {
    let by_id: HashMap<&str, &Node> = s.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let page_of = |id: &str| {
        let mut node = by_id[id];
        while let Some(parent) = &node.parent_id {
            node = by_id[parent.as_str()];
        }
        node.id.clone()
    };
    let pick = |rng: &mut Rng, keep: &dyn Fn(&Node) -> bool| {
        let all: Vec<&Node> = s.nodes.iter().filter(|n| keep(n)).collect();
        (!all.is_empty()).then(|| all[rng.below(all.len())])
    };
    let parents: HashSet<&str> = s
        .nodes
        .iter()
        .filter_map(|n| n.parent_id.as_deref())
        .collect();
    let has_children = |n: &Node| parents.contains(n.id.as_str());
    let not_page = |n: &Node| n.parent_id.is_some();
    let frame = |n: &Node| n.kind == NodeKind::Frame;
    let new_id = format!("shadow_rd_{serial}");
    let one = |label, value| Batch {
        label,
        ops: vec![op(value)],
        pages: None,
    };
    Some(match step % KINDS {
        0 => {
            let n = pick(rng, &not_page)?;
            one(
                "rename",
                json!({"op":"update_node","id":n.id,"props":{"name":format!("shadow {serial}")}}),
            )
        }
        1 => {
            let n = pick(rng, &|n| n.props.as_object().is_some_and(|p| !p.is_empty()))?;
            let (key, value) = n.props.as_object()?.iter().next()?;
            one(
                "noop",
                json!({"op":"update_node","id":n.id,"props":{key: value}}),
            )
        }
        2 => {
            let n = pick(rng, &|n| {
                n.props.get("layout").is_some_and(Value::is_object)
            })?;
            let gap = n.props["layout"].get("gap").cloned().unwrap_or(json!(0));
            one(
                "merge",
                json!({"op":"update_node","id":n.id,"props":{"layout":{"gap":gap}},"merge":true}),
            )
        }
        3 => {
            let n = pick(rng, &not_page)?;
            one("reorder", json!({"op":"reorder_node","id":n.id,"index":0}))
        }
        4 => {
            let parent = pick(rng, &frame)?;
            one(
                "create",
                json!({"op":"create_node","id":new_id,"kind":"rectangle","parentId":parent.id,
                "props":{"width":4,"height":4},"index":rng.below(3)}),
            )
        }
        5 => {
            let n = pick(rng, &|n| n.id.starts_with("shadow_rd_"))?;
            one("delete_new", json!({"op":"delete_subtree","id":n.id}))
        }
        6 => {
            let n = pick(rng, &not_page)?;
            Batch {
                label: "note",
                pages: None,
                ops: vec![
                    op(json!({"op":"create_note","id":new_id,"nodeId":n.id,"text":"shadow check"})),
                    op(json!({"op":"reply_note","id":new_id,"text":"ok"})),
                ],
            }
        }
        7 => {
            let note = s.notes.iter().find(|n| n.id.starts_with("shadow_rd_"))?;
            one("delete_note", json!({"op":"delete_note","id":note.id}))
        }
        8 => {
            let n = pick(rng, &|n| not_page(n) && !has_children(n))?;
            let page = page_of(&n.id);
            let target = pick(rng, &|f| frame(f) && page_of(&f.id) == page && f.id != n.id)?;
            one(
                "reparent",
                json!({"op":"reparent_node","id":n.id,"parentId":target.id}),
            )
        }
        9 => {
            let n = pick(rng, &|n| not_page(n) && !has_children(n))?;
            one("delete_leaf", json!({"op":"delete_subtree","id":n.id}))
        }
        10 => {
            let master = pick(rng, &|n| frame(n) && n.props.get("component").is_some())?;
            let child = s
                .nodes
                .iter()
                .find(|c| c.parent_id.as_ref() == Some(&master.id))?;
            one(
                "master_child",
                json!({"op":"update_node","id":child.id,"props":{"name":format!("m {serial}")}}),
            )
        }
        11 => {
            let n = pick(rng, &not_page)?;
            let mut b = one(
                "scoped",
                json!({"op":"update_node","id":n.id,"props":{"name":format!("s {serial}")}}),
            );
            b.pages = Some(vec![page_of(&n.id)]);
            b
        }
        12 => {
            let token = &s.tokens.get(rng.below(s.tokens.len()))?;
            one("token_noop", json!({"op":"upsert_token","token":token}))
        }
        _ => unreachable!("step is taken modulo KINDS"),
    })
}
