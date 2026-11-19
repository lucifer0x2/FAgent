//! Component references form a directed, bounded DAG independent of authored parents.
//! An edge `M -> T` exists when a component master `M` contains (within 128 ancestors)
//! an instance of `T`. The walk ([`Walk`]) is shared by full validation and the delta
//! rules (`delta::graph`), which fill edges lazily from a [`Lookup`].
use super::{Candidate, Lookup};
use crate::{Node, NodeKind, Result, StorageError};
use std::collections::{BTreeMap, BTreeSet};

impl Candidate {
    pub(super) fn instance_graph(&self) -> Result<()> {
        let instances = self.nodes.values();
        walk_all(self, instances.filter(|n| n.kind == NodeKind::Instance))
    }
}

/// The full check over `instances` (every authored instance): an edge from every
/// master within 128 ancestors of an instance to its target, every master with an edge
/// visited in id order with one shared memo.
pub(super) fn walk_all<'a>(
    lookup: &dyn Lookup,
    instances: impl Iterator<Item = &'a Node>,
) -> Result<()> {
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for node in instances {
        let Some(target) = node.props["componentId"].as_str() else {
            continue;
        };
        for master in enclosing_masters(lookup, node.parent_id.as_deref()) {
            edges.entry(master).or_default().insert(target.to_owned());
        }
    }
    let roots: Vec<String> = edges.keys().cloned().collect();
    let mut walk = Walk::known(lookup, edges);
    for root in &roots {
        walk.visit(root, &mut BTreeSet::new())?;
    }
    Ok(())
}

/// Component masters among `parent` and its ancestors (bounded walk).
pub(super) fn enclosing_masters(lookup: &dyn Lookup, parent: Option<&str>) -> Vec<String> {
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

/// Depth-first walk of the component graph with a height memo.
pub(super) struct Walk<'l> {
    lookup: &'l dyn Lookup,
    edges: BTreeMap<String, BTreeSet<String>>,
    /// Whether `edges` holds every edge (a missing master has none) or is filled
    /// lazily from the master's subtree.
    complete: bool,
    heights: BTreeMap<String, usize>,
}

impl<'l> Walk<'l> {
    /// A walk over every edge, collected up front.
    fn known(lookup: &'l dyn Lookup, edges: BTreeMap<String, BTreeSet<String>>) -> Self {
        Self {
            lookup,
            edges,
            complete: true,
            heights: BTreeMap::new(),
        }
    }

    /// A walk that reads a master's edges from its subtree on first visit.
    pub(super) fn lazy(lookup: &'l dyn Lookup) -> Self {
        Self {
            complete: false,
            ..Self::known(lookup, BTreeMap::new())
        }
    }

    /// Targets of the instances strictly inside master `id`; none for a non-master.
    fn edges(&mut self, id: &str) -> BTreeSet<String> {
        if let Some(edges) = self.edges.get(id) {
            return edges.clone();
        }
        let mut targets = BTreeSet::new();
        let master = self.lookup.node(id);
        if !self.complete && master.is_some_and(|m| m.props.get("component").is_some()) {
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

    /// The height of `id`, or the cycle or depth error.
    pub(super) fn visit(&mut self, id: &str, path: &mut BTreeSet<String>) -> Result<usize> {
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
