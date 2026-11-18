//! The component graph check (`Candidate::instance_graph`) around the masters a batch
//! changed. An edge `M -> T` exists when a component master `M` contains an instance of
//! `T`. On a valid base the graph was acyclic and at most 8 deep, so a new cycle or a
//! new too-long path runs through a master whose edges changed (a *start* master). The
//! full check walks from every master in id order; only the masters that reach a start
//! master (its users' enclosing masters, transitively) can fail. Which error that walk
//! meets first (cycle or depth) depends on the masters walked before, so on a fault the
//! graph is walked once more over every master in id order, as the full check does;
//! accepted batches never pay for that walk.
use crate::validate::Lookup;
use crate::{NodeKind, Result, StorageError};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn check<'a>(after: &dyn Lookup, affected: impl Iterator<Item = &'a str>) -> Result<()> {
    let mut start = BTreeSet::new();
    for node in affected.filter_map(|id| after.node(id)) {
        if node.props.get("component").is_some() {
            start.insert(node.id.clone());
        }
        if node.kind == NodeKind::Instance {
            start.extend(enclosing_masters(after, node.parent_id.as_deref()));
        }
    }
    let mut roots = BTreeSet::new();
    let mut pending: Vec<String> = start.into_iter().collect();
    while let Some(master) = pending.pop() {
        if !roots.insert(master.clone()) {
            continue;
        }
        for user in after.component_users(&master) {
            let parent = after.node(user).and_then(|n| n.parent_id.as_deref());
            pending.extend(enclosing_masters(after, parent));
        }
    }
    let mut walk = Walk {
        lookup: after,
        edges: BTreeMap::new(),
        heights: BTreeMap::new(),
    };
    for root in &roots {
        if let Err(fault) = walk.visit(root, &mut BTreeSet::new()) {
            return Err(full(after).err().unwrap_or(fault));
        }
    }
    Ok(())
}

/// `Candidate::instance_graph` over the whole project: an edge from every master within
/// 128 ancestors of an instance to its target, every master with an edge visited in id
/// order with one shared memo.
fn full(lookup: &dyn Lookup) -> Result<()> {
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut pending = lookup.pages();
    while let Some(node) = pending.pop() {
        pending.extend(lookup.children(&node.id));
        if node.props.get("component").is_some() {
            // Known edges only: no descent below the 128-ancestor bound.
            edges.entry(node.id.clone()).or_default();
        }
        if node.kind != NodeKind::Instance {
            continue;
        }
        let Some(target) = node.props["componentId"].as_str() else {
            continue;
        };
        for master in enclosing_masters(lookup, node.parent_id.as_deref()) {
            edges.entry(master).or_default().insert(target.to_owned());
        }
    }
    let roots: Vec<String> = edges
        .iter()
        .filter(|(_, targets)| !targets.is_empty())
        .map(|(id, _)| id.clone())
        .collect();
    let mut walk = Walk {
        lookup,
        edges,
        heights: BTreeMap::new(),
    };
    for root in &roots {
        walk.visit(root, &mut BTreeSet::new())?;
    }
    Ok(())
}

/// Component masters among `parent` and its ancestors (bounded walk, as the full
/// check's).
fn enclosing_masters(lookup: &dyn Lookup, parent: Option<&str>) -> Vec<String> {
    let mut current = parent.and_then(|id| lookup.node(id));
    let mut masters = Vec::new();
    for _ in 0..128 {
        let Some(node) = current else { break };
        if node.props.get("component").is_some() {
            masters.push(node.id.clone());
        }
        current = node.parent_id.as_deref().and_then(|id| lookup.node(id));
    }
    masters
}

struct Walk<'l> {
    lookup: &'l dyn Lookup,
    edges: BTreeMap<String, BTreeSet<String>>,
    heights: BTreeMap<String, usize>,
}

impl Walk<'_> {
    /// Targets of the instances strictly inside master `id`; none for a non-master.
    fn edges(&mut self, id: &str) -> BTreeSet<String> {
        if let Some(edges) = self.edges.get(id) {
            return edges.clone();
        }
        let mut targets = BTreeSet::new();
        let master = self.lookup.node(id);
        if master.is_some_and(|m| m.props.get("component").is_some()) {
            let mut pending = vec![id.to_owned()];
            while let Some(parent) = pending.pop() {
                for child in self.lookup.children(&parent) {
                    if child.kind == NodeKind::Instance {
                        if let Some(target) = child.props["componentId"].as_str() {
                            targets.insert(target.to_owned());
                        }
                    }
                    pending.push(child.id.clone());
                }
            }
        }
        self.edges.insert(id.to_owned(), targets.clone());
        targets
    }

    /// `instance_graph`'s visit: the height of `id`, or the cycle or depth error.
    fn visit(&mut self, id: &str, path: &mut BTreeSet<String>) -> Result<usize> {
        if let Some(height) = self.heights.get(id) {
            return Ok(*height);
        }
        if !path.insert(id.to_owned()) {
            return Err(StorageError::new(
                "INSTANCE_CYCLE",
                "Component reference cycle detected",
            ));
        }
        if path.len() > 8 {
            return Err(depth());
        }
        let mut height = 1;
        for target in self.edges(id) {
            height = height.max(1 + self.visit(&target, path)?);
        }
        if height > 8 {
            return Err(depth());
        }
        path.remove(id);
        self.heights.insert(id.to_owned(), height);
        Ok(height)
    }
}

fn depth() -> StorageError {
    StorageError::new("INSTANCE_DEPTH", "Component nesting exceeds 8")
}
