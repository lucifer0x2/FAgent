//! Bulk build of the persistent state from a snapshot: intern ids, link parents, sort
//! sibling lists, derive page/root/master in preorder, then build each map once.
use super::pmap::{Family, PMap};
use super::state::{Interner, Rec, Set, State};
use fagent_scene_input::{NodeKind, NodeProps, Snapshot};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub fn build<F: Family>(snapshot: Snapshot) -> (Interner, State<F>) {
    let nodes = snapshot.nodes;
    let n = nodes.len();
    let mut interner = Interner::default();
    for node in &nodes {
        interner.intern(&node.id);
    }
    let parents: Vec<Option<u32>> = nodes
        .iter()
        .map(|node| node.parent_id.as_deref().and_then(|p| interner.get(p)))
        .collect();
    let mut lists: Vec<Vec<u32>> = vec![Vec::new(); n];
    let mut pages = Vec::new();
    for (key, parent) in parents.iter().enumerate() {
        match parent {
            Some(p) => lists[*p as usize].push(key as u32),
            None => pages.push(key as u32),
        }
    }
    let order = |k: &u32| (nodes[*k as usize].sort_key, nodes[*k as usize].id.as_str());
    pages.sort_by(|a, b| order(a).cmp(&order(b)));
    for list in &mut lists {
        list.sort_by(|a, b| order(a).cmp(&order(b)));
    }
    // (page, root, master) by preorder from each page.
    let mut derived = vec![(0u32, None, None); n];
    let mut stack: Vec<u32> = pages.iter().rev().copied().collect();
    while let Some(key) = stack.pop() {
        let node = &nodes[key as usize];
        let own = (node.kind == NodeKind::Frame
            && NodeProps::new(&node.props).component().is_some())
        .then_some(key);
        derived[key as usize] = match parents[key as usize] {
            None => (key, None, own),
            Some(p) => {
                let (page, root, master) = derived[p as usize];
                let root = match parents[p as usize] {
                    None => Some(key),
                    Some(_) => root,
                };
                (page, root, master.or(own))
            }
        };
        stack.extend(lists[key as usize].iter().rev());
    }
    let mut users: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    let mut links: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for (key, node) in nodes.iter().enumerate() {
        let props = NodeProps::new(&node.props);
        if node.kind == NodeKind::Instance {
            if let Some(master) = props.component_id() {
                let master = interner.intern(master);
                users.entry(master).or_default().insert(key as u32);
            }
        }
        for target in props.link_targets() {
            let target = interner.intern(target);
            links.entry(target).or_default().insert(key as u32);
        }
    }
    let records: Vec<(u32, Rec)> = nodes
        .into_iter()
        .enumerate()
        .map(|(key, node)| {
            let (page, root, master) = derived[key];
            let rec = Rec {
                node: Arc::new(node),
                parent: parents[key],
                page,
                root,
                master,
            };
            (key as u32, rec)
        })
        .collect();
    let children = lists
        .into_iter()
        .enumerate()
        .filter(|(_, list)| !list.is_empty())
        .map(|(key, list)| (key as u32, Arc::new(list)))
        .collect();
    let sets = |map: BTreeMap<u32, BTreeSet<u32>>| -> Vec<(u32, Set)> {
        map.into_iter().map(|(k, v)| (k, Arc::new(v))).collect()
    };
    let state = State {
        nodes: F::Map::build(records),
        children: F::Map::build(children),
        pages: Arc::new(pages),
        users: F::Map::build(sets(users)),
        links: F::Map::build(sets(links)),
        revision: snapshot.revision,
        len: n,
    };
    (interner, state)
}
