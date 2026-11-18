//! Scoped traversal over a view's ids, without slice or positional-map rebuilds.
use super::{ReadScope, ReadView, ScopeStats, ScopedNodes, ScopedSnapshot};
use crate::{Node, NodeKind, Result, StorageError};
use std::collections::BTreeSet;

impl ReadView {
    fn descendants<'a>(&'a self, id: &str, selected: &mut BTreeSet<&'a str>) {
        let Some(root) = self.node(id) else {
            return;
        };
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            if !selected.insert(node.id.as_str()) {
                continue;
            }
            pending.extend(self.source.children(&node.id));
        }
    }

    fn ancestors<'a>(&'a self, id: &str, selected: &mut BTreeSet<&'a str>) {
        let mut node = self.node(id);
        for _ in 0..=128 {
            let Some(current) = node else {
                break;
            };
            selected.insert(current.id.as_str());
            node = current.parent_id.as_deref().and_then(|id| self.node(id));
        }
    }

    fn select(&self, scope: ReadScope<'_>) -> Result<BTreeSet<&str>> {
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

    fn masters<'a>(&'a self, selected: &BTreeSet<&'a str>) -> BTreeSet<&'a str> {
        let mut closure = selected.clone();
        let mut scanned = BTreeSet::new();
        loop {
            let pending: Vec<&str> = closure.difference(&scanned).copied().collect();
            if pending.is_empty() {
                return closure;
            }
            for id in pending {
                scanned.insert(id);
                if let Some(master) = self.master(id) {
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
        for source in self.source.linking(frame) {
            self.ancestors(source, &mut selected);
        }
        self.ordered(selected)
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
        let authored = view.ordered(match definitions {
            true => closure.iter().copied(),
            false => selected.iter().copied(),
        });
        let dependencies = view.ordered(closure.iter().copied());
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
        let authored = view.ordered(selected.iter().copied());
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
