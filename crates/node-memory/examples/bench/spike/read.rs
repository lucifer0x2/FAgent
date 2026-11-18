//! The structural part of a `get_design_context` read of one frame, the same steps as
//! `ReadView::expand_view` (`scene-input/src/scoped/view_reads.rs`): the target subtree
//! and its ancestors, the master closure through `componentId` (subtrees and
//! ancestors), ordered by `(parent_id, sort_key, id)`. Instance expansion and property
//! resolution come after and do not depend on the store, so they are left out.
use super::pmap::Family;
use super::state::{Interner, State};
use fagent_scene_input::read_scope::ReadView;
use fagent_scene_input::{Node, NodeProps};
use std::collections::{BTreeSet, HashSet};

fn ordered(mut nodes: Vec<&Node>) -> Vec<&Node> {
    nodes.sort_by(|a, b| (&a.parent_id, a.sort_key, &a.id).cmp(&(&b.parent_id, b.sort_key, &b.id)));
    nodes
}

/// Options (b) and (c): interned keys and a hash set.
pub fn frame<'s, F: Family>(state: &'s State<F>, interner: &Interner, id: &str) -> Vec<&'s Node> {
    let Some(target) = interner.get(id).filter(|k| state.rec(*k).is_some()) else {
        return Vec::new();
    };
    let mut closure = HashSet::new();
    let mut scan = Vec::new();
    let add = |key: u32, closure: &mut HashSet<u32>, scan: &mut Vec<u32>| {
        for k in state.subtree(key) {
            if closure.insert(k) {
                scan.push(k);
            }
        }
        let mut up = state.rec(key).and_then(|r| r.parent);
        for _ in 0..=128 {
            let Some(k) = up else { break };
            if closure.insert(k) {
                scan.push(k);
            }
            up = state.rec(k).and_then(|r| r.parent);
        }
    };
    add(target, &mut closure, &mut scan);
    while let Some(key) = scan.pop() {
        let node = &state.rec(key).expect("live").node;
        let master = NodeProps::new(&node.props).component_id();
        if let Some(master) = master
            .and_then(|m| interner.get(m))
            .filter(|m| state.rec(*m).is_some())
        {
            if !closure.contains(&master) {
                add(master, &mut closure, &mut scan);
            }
        }
    }
    ordered(
        closure
            .into_iter()
            .map(|k| &*state.rec(k).expect("live").node)
            .collect(),
    )
}

/// Option (a): the published `ReadView`, through its public accessors (string ids).
pub fn frame_view<'v>(view: &'v ReadView, id: &str) -> Vec<&'v Node> {
    let mut closure = BTreeSet::new();
    let add = |id: &str, closure: &mut BTreeSet<String>| {
        let mut pending = vec![id.to_owned()];
        while let Some(id) = pending.pop() {
            if closure.insert(id.clone()) {
                pending.extend(view.children(&id).map(str::to_owned));
            }
        }
        let mut node = view
            .node(id)
            .and_then(|n| n.parent_id.as_deref())
            .and_then(|p| view.node(p));
        for _ in 0..=128 {
            let Some(current) = node else { break };
            closure.insert(current.id.clone());
            node = current.parent_id.as_deref().and_then(|p| view.node(p));
        }
    };
    add(id, &mut closure);
    let mut scanned = BTreeSet::new();
    loop {
        let pending: Vec<String> = closure.difference(&scanned).cloned().collect();
        if pending.is_empty() {
            break;
        }
        for id in pending {
            let master = view
                .node(&id)
                .and_then(|n| NodeProps::new(&n.props).component_id());
            if let Some(master) = master.filter(|m| view.node(m).is_some()) {
                add(master, &mut closure);
            }
            scanned.insert(id);
        }
    }
    ordered(closure.iter().filter_map(|id| view.node(id)).collect())
}
