//! Scoped slice: a small snapshot holding everything the scoped readers
//! (`read_scope::ScopedSnapshot`) touch for one target, so they answer on the slice
//! exactly as on the whole project. Work is proportional to the slice, not the project.
use super::ProjectState;
use crate::props::{component_id, link_targets};
use crate::store::{Key, Store};
use fagent_scene_input::{Node, Snapshot};
use std::collections::{BTreeSet, HashMap, HashSet};

struct Builder<'a> {
    store: &'a Store,
    selected: HashSet<Key>,
    /// Nodes inside an included whole subtree.
    covered: HashSet<Key>,
    /// Roots of included whole subtrees.
    full: HashSet<Key>,
    /// Closure nodes whose `componentId` is not followed yet.
    scan: Vec<Key>,
}

impl<'a> Builder<'a> {
    /// Adds `key` and its ancestors. The selection stays ancestor-closed, so the walk
    /// stops at the first ancestor already selected.
    fn path(&mut self, key: Key, scan: bool) {
        for k in std::iter::once(key).chain(self.store.ancestors(key)) {
            if !self.selected.insert(k) {
                return;
            }
            if scan {
                self.scan.push(k);
            }
        }
    }

    /// Adds the whole subtree of `root` (scanned for masters) and its ancestors.
    fn subtree(&mut self, root: Key) {
        if self.covered.contains(&root) {
            return;
        }
        self.full.insert(root);
        for key in self.store.subtree_keys(root) {
            self.covered.insert(key);
            if self.selected.insert(key) {
                self.scan.push(key);
            }
        }
        if let Some(parent) = self.store.slot(root).parent {
            self.path(parent, true);
        }
    }

    /// Masters reached through `componentId` (any node kind, as the readers follow
    /// it), recursively: their subtrees and ancestors.
    fn masters(&mut self) {
        while let Some(key) = self.scan.pop() {
            let master = component_id(&self.store.node(key).props).and_then(|m| self.store.key(m));
            if let Some(master) = master {
                self.subtree(master);
            }
        }
    }

    /// Navigation metadata: ancestors of link targets of the closure, and of nodes
    /// linking to any node of the target's subtree. Not followed for masters.
    fn navigation(&mut self, target: Key) {
        let closure: Vec<Key> = self.selected.iter().copied().collect();
        for key in closure {
            let targets: Vec<Key> = link_targets(&self.store.node(key).props)
                .filter_map(|id| self.store.key(id))
                .collect();
            for linked in targets {
                self.path(linked, false);
            }
        }
        for key in self.store.subtree_keys(target) {
            let sources: Vec<Key> = self.store.linking(&self.store.node(key).id).collect();
            for source in sources {
                self.path(source, false);
            }
        }
    }

    /// Selected nodes in project order (pages, then preorder by sibling order).
    fn nodes(&self) -> Vec<Node> {
        let mut groups: HashMap<Option<Key>, Vec<Key>> = HashMap::new();
        for &key in &self.selected {
            if self.covered.contains(&key) && !self.full.contains(&key) {
                continue;
            }
            groups
                .entry(self.store.slot(key).parent)
                .or_default()
                .push(key);
        }
        for list in groups.values_mut() {
            list.sort_by(|a, b| self.store.sibling_cmp(*a, *b));
        }
        let mut out = Vec::with_capacity(self.selected.len());
        let mut stack: Vec<Key> = groups
            .get(&None)
            .into_iter()
            .flatten()
            .rev()
            .copied()
            .collect();
        while let Some(key) = stack.pop() {
            if self.full.contains(&key) {
                out.extend(
                    self.store
                        .subtree_keys(key)
                        .into_iter()
                        .map(|k| self.store.node(k).clone()),
                );
                continue;
            }
            out.push(self.store.node(key).clone());
            let children = groups.get(&Some(key)).into_iter().flatten().rev();
            stack.extend(children.copied());
        }
        out
    }
}

impl ProjectState {
    /// A snapshot for scoped reads of node `id` (normally a root frame) and of any
    /// target inside its subtree: that subtree with its ancestors; masters reached
    /// through `componentId`, recursively, with their subtrees and ancestors; ancestors
    /// of link targets and of nodes linking into the subtree; notes anchored to any
    /// included node; every token, style, asset and font. Nodes keep project order.
    /// `None` for an unknown id.
    pub fn slice(&self, id: &str) -> Option<Snapshot> {
        let target = self.store.key(id)?;
        let mut builder = Builder {
            store: &self.store,
            selected: HashSet::new(),
            covered: HashSet::new(),
            full: HashSet::new(),
            scan: Vec::new(),
        };
        builder.subtree(target);
        builder.masters();
        builder.navigation(target);
        let mut snapshot = self.meta.as_ref().clone();
        snapshot.nodes = builder.nodes();
        let res = &self.store.res;
        let notes: BTreeSet<&str> = builder
            .selected
            .iter()
            .flat_map(|&k| res.notes_on(&self.store.node(k).id))
            .collect();
        snapshot.notes = notes
            .into_iter()
            .map(|n| res.notes[n].as_ref().clone())
            .collect();
        Some(snapshot)
    }
}
