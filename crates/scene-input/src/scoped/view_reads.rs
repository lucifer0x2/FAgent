//! Scoped traversal over persistent IDs, without slice or positional-map rebuilds.
use super::{ReadScope, ReadView, ScopeStats, ScopedNodes, ScopedSnapshot};
use crate::{Node, NodeKind, Result, StorageError};
use std::collections::BTreeSet;

impl ReadView {
    fn descendants(&self, id: &str, selected: &mut BTreeSet<String>) {
        let mut pending = vec![id.to_owned()];
        while let Some(id) = pending.pop() {
            if !selected.insert(id.clone()) {
                continue;
            }
            if let Some(children) = self.index.children.get(&id) {
                pending.extend(children.iter().map(|(_, id)| id.clone()));
            }
        }
    }

    fn ancestors(&self, id: &str, selected: &mut BTreeSet<String>) {
        let mut node = self.node(id);
        for _ in 0..=128 {
            let Some(current) = node else {
                break;
            };
            selected.insert(current.id.clone());
            node = current.parent_id.as_deref().and_then(|id| self.node(id));
        }
    }

    fn select(&self, scope: ReadScope<'_>) -> Result<BTreeSet<String>> {
        let (id, is_page) = match scope {
            ReadScope::Page(id) => (id, true),
            ReadScope::Target(id) => (id, false),
        };
        let id = id.split('/').next().unwrap_or(id);
        let node = self
            .node(id)
            .filter(|_| self.page(id).is_some())
            .ok_or_else(|| StorageError::new("INVALID_REQUEST", "Scope node does not exist"))?;
        if is_page && node.kind != NodeKind::Page {
            return Err(StorageError::new(
                "INVALID_REQUEST",
                "Scope node must identify a page",
            ));
        }
        let mut selected = BTreeSet::new();
        self.descendants(id, &mut selected);
        self.ancestors(id, &mut selected);
        Ok(selected)
    }

    fn masters(&self, selected: &BTreeSet<String>) -> BTreeSet<String> {
        let mut closure = selected.clone();
        let mut scanned = BTreeSet::new();
        loop {
            let pending: Vec<_> = closure.difference(&scanned).cloned().collect();
            if pending.is_empty() {
                return closure;
            }
            for id in pending {
                scanned.insert(id.clone());
                if let Some(master) = self.index.masters.get(&id) {
                    self.descendants(master, &mut closure);
                    self.ancestors(master, &mut closure);
                }
            }
        }
    }

    pub(super) fn navigation<'a>(&'a self, nodes: &[&Node], frame: &str) -> Vec<&'a Node> {
        let mut selected = BTreeSet::new();
        for node in nodes {
            for link in crate::node_links(&node.props) {
                if let Some(target) = link.to {
                    self.ancestors(&target, &mut selected);
                }
            }
        }
        for source in self
            .index
            .incoming
            .get(frame)
            .into_iter()
            .flat_map(|ids| ids.iter())
        {
            self.ancestors(source, &mut selected);
        }
        self.ordered_ids(&selected)
    }

    fn ordered_ids<'a>(&'a self, ids: &BTreeSet<String>) -> Vec<&'a Node> {
        let mut nodes: Vec<_> = ids.iter().filter_map(|id| self.node(id)).collect();
        nodes.sort_by(|a, b| {
            (&a.parent_id, a.sort_key, &a.id).cmp(&(&b.parent_id, b.sort_key, &b.id))
        });
        nodes
    }
}

impl<'a> ScopedSnapshot<'a> {
    pub(super) fn expand_view(
        &self,
        view: &'a ReadView,
        scope: ReadScope<'_>,
        definitions: bool,
    ) -> Result<ScopedNodes<'a>> {
        let selected = view.select(scope)?;
        let closure = view.masters(&selected);
        let authored = view.ordered_ids(if definitions { &closure } else { &selected });
        let dependencies = view.ordered_ids(&closure);
        let expanded = self.expand_authored(scope_id(scope), &authored)?;
        let stats = ScopeStats {
            closure_nodes: closure.len(),
            expanded_nodes: expanded.len(),
            resolved_nodes: 0,
        };
        Ok(ScopedNodes {
            authored,
            dependencies,
            expanded,
            stats,
        })
    }

    pub(super) fn expand_view_nodes(
        &self,
        view: &'a ReadView,
        scope: ReadScope<'_>,
    ) -> Result<(Vec<Node>, ScopeStats)> {
        let selected = view.select(scope)?;
        let authored = view.ordered_ids(&selected);
        let expanded = self.expand_authored(scope_id(scope), &authored)?;
        let stats = ScopeStats {
            closure_nodes: selected.len(),
            expanded_nodes: expanded.len(),
            resolved_nodes: 0,
        };
        Ok((expanded, stats))
    }
}

fn scope_id(scope: ReadScope<'_>) -> &str {
    match scope {
        ReadScope::Page(id) | ReadScope::Target(id) => id,
    }
}
