//! Counters the delta rules keep between revisions: the expanded size of the project,
//! so the expanded-node limit is checked without expanding every instance.
//!
//! The counter is kept per component master: the virtual nodes one instance of it adds
//! (its descendants, each nested instance counting its own master's size instead of
//! its authored children). The total is the authored count plus, for every authored
//! instance, its master's size. A batch recomputes the masters around its closure from
//! authored rows only (no virtual node is built) and adjusts the total for the instances
//! in its closure, which holds every user of a master whose size can have changed.
use crate::read_scope::persistent::Map;
use crate::validate::Lookup;
use crate::{Node, NodeKind, StorageError};
use std::collections::{BTreeMap, BTreeSet};

/// Expanded size of one revision: virtual nodes per instance of each component master
/// and the total (authored nodes plus virtual nodes). Cloning is cheap; an update
/// copies one shard per changed master.
#[derive(Clone, Default)]
pub struct Facts {
    sizes: Map<usize>,
    total: usize,
}

impl Facts {
    /// Authored plus virtual nodes.
    pub fn total(&self) -> usize {
        self.total
    }

    pub fn estimated_bytes(&self) -> usize {
        self.sizes.estimated_bytes()
    }

    /// The facts after a batch: `affected` lists the closure (deleted nodes included),
    /// which holds every instance whose expansion may have changed. Fails when the
    /// expanded project exceeds `max_nodes`.
    pub(super) fn update<'a>(
        &self,
        (before, after): (&dyn Lookup, &dyn Lookup),
        affected: impl Iterator<Item = &'a str> + Clone,
        max_nodes: usize,
    ) -> crate::Result<Self> {
        let mut changed = BTreeSet::new();
        for id in affected.clone() {
            for view in [before, after] {
                let masters = super::affected::component_ancestors(view, id);
                changed.extend(masters.into_iter().map(str::to_owned));
            }
        }
        let mut next = self.clone();
        let mut sizes = Sizes {
            lookup: after,
            old: &self.sizes,
            changed: &changed,
            memo: BTreeMap::new(),
        };
        let mut total = (self.total + after.node_count()).saturating_sub(before.node_count());
        for id in affected {
            if let Some(master) = before.node(id).and_then(target) {
                total = total.saturating_sub(self.size(before, master));
            }
            if let Some(master) = after.node(id).and_then(target) {
                total = total.saturating_add(sizes.of(master));
            }
        }
        for master in &changed {
            next.sizes.remove(master);
        }
        for (master, size) in sizes.memo {
            if let Some(size) = size {
                next.sizes.insert(master.to_owned(), size);
            }
        }
        next.total = total;
        if next.total > max_nodes {
            return Err(StorageError::new(
                "INSTANCE_LIMIT",
                &format!("Expanded project exceeds {max_nodes} nodes"),
            ));
        }
        Ok(next)
    }

    /// The stored size of `master` in the revision these facts describe, computed over
    /// `lookup` (that revision) when no size was stored.
    fn size(&self, lookup: &dyn Lookup, master: &str) -> usize {
        if let Some(&size) = self.sizes.get(master) {
            return size;
        }
        let none = BTreeSet::new();
        let mut sizes = Sizes {
            lookup,
            old: &self.sizes,
            changed: &none,
            memo: BTreeMap::new(),
        };
        sizes.of(master)
    }
}

/// The facts of a whole revision, given every authored instance id. An instance whose
/// master is missing or no component frame (stored data admitted although invalid)
/// counts no virtual nodes, nor does a cycle among masters.
pub fn facts<'a>(lookup: &dyn Lookup, instances: impl Iterator<Item = &'a str>) -> Facts {
    let empty = Map::default();
    let none = BTreeSet::new();
    let mut sizes = Sizes {
        lookup,
        old: &empty,
        changed: &none,
        memo: BTreeMap::new(),
    };
    let mut total = lookup.node_count();
    for node in instances.filter_map(|id| lookup.node(id)) {
        if let Some(master) = target(node) {
            total += sizes.of(master);
        }
    }
    let mut facts = Facts {
        total,
        ..Facts::default()
    };
    for (master, size) in sizes.memo {
        if let Some(size) = size {
            facts.sizes.insert(master.to_owned(), size);
        }
    }
    facts
}

/// The master an instance expands (`componentId`), if `node` is an instance.
fn target(node: &Node) -> Option<&str> {
    (node.kind == NodeKind::Instance)
        .then(|| node.props["componentId"].as_str())
        .flatten()
}

/// Master sizes over one revision: stored sizes for the masters the batch cannot have
/// changed, computed (and memoized) for the others.
struct Sizes<'a> {
    lookup: &'a dyn Lookup,
    old: &'a Map<usize>,
    changed: &'a BTreeSet<String>,
    /// Computed sizes; `None` while a master is being computed (a cycle counts 0).
    memo: BTreeMap<&'a str, Option<usize>>,
}

impl<'a> Sizes<'a> {
    fn of(&mut self, master: &str) -> usize {
        if let Some(size) = self.memo.get(master) {
            return size.unwrap_or(0);
        }
        if !self.changed.contains(master) {
            if let Some(&size) = self.old.get(master) {
                return size;
            }
        }
        let Some(node) = self.lookup.node(master) else {
            return 0;
        };
        if node.kind != NodeKind::Frame || node.props.get("component").is_none() {
            return 0;
        }
        let id = node.id.as_str();
        self.memo.insert(id, None);
        let size = self.subtree(id, 1);
        self.memo.insert(id, Some(size));
        size
    }

    /// Virtual nodes below `parent` in an expansion: one per descendant, a nested
    /// instance adding its master's size instead of its own children (bounded depth).
    fn subtree(&mut self, parent: &str, level: usize) -> usize {
        if level > crate::instances::MAX_TREE_DEPTH {
            return 0;
        }
        let mut size = 0;
        for child in self.lookup.children(parent) {
            size += 1;
            size += match target(child) {
                Some(master) => self.of(master),
                None if child.kind == NodeKind::Instance => 0,
                None => self.subtree(&child.id, level + 1),
            };
        }
        size
    }
}

#[cfg(test)]
#[path = "facts_tests.rs"]
mod tests;
