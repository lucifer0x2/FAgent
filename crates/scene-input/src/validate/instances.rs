//! Component graph, override shape and authored/virtual write boundaries.
use super::instance_checks::{check_expanded, check_instance_node};
use super::Candidate;
use crate::{instances::expand, *};
use std::collections::{BTreeMap, BTreeSet};

impl Candidate {
    /// Component graph, expansion and instance checks of `affected` nodes (all when
    /// `None`); returns the expansion, or `None` when no instance or master exists.
    pub(super) fn validate_instances(
        &self,
        affected: Option<&BTreeSet<String>>,
        themes: &BTreeSet<String>,
    ) -> Result<Option<BTreeMap<String, Node>>> {
        if !self
            .nodes
            .values()
            .any(|n| n.kind == NodeKind::Instance || n.props.get("component").is_some())
        {
            return Ok(None);
        }
        let has_instances = self.nodes.values().any(|n| n.kind == NodeKind::Instance);
        self.instance_graph()?;
        let expanded = crate::bench_phase::time("validate.instances.expand", || {
            expand(&self.nodes.values().cloned().collect::<Vec<_>>())
        })?;
        let order: Vec<_> = expanded.iter().map(|node| node.id.clone()).collect();
        let placed: BTreeMap<_, _> = expanded.into_iter().map(|n| (n.id.clone(), n)).collect();
        let selected = |id: &str| affected.is_none_or(|ids| ids.contains(id));
        for node in self.nodes.values().filter(|node| selected(&node.id)) {
            check_instance_node(self, node, &placed, themes)?;
        }
        if has_instances {
            // The bounded expander rejects direct/indirect cycles and depth explosions.
            // Preserve the expander's authored-ID/depth-first order, rather than
            // BTreeMap's virtual-ID order, for failure precedence.
            let order = order.iter().map(|id| &placed[id]).filter(|n| {
                (n.id.contains('/')
                    || self
                        .nodes
                        .get(&n.id)
                        .is_some_and(|n| n.kind == NodeKind::Instance))
                    && selected(n.id.split('/').next().unwrap())
            });
            check_expanded(self, &placed, order, themes)?;
        }
        Ok(Some(placed))
    }

    pub(super) fn instance_write_guard(&self, op: &Operation) -> Result<()> {
        let parent = match op {
            Operation::CreateNode { parent_id, .. } => parent_id.as_deref(),
            Operation::ReparentNode { parent_id, .. } => Some(parent_id.as_str()),
            _ => None,
        };
        let id = match op {
            Operation::CreateNode { id, .. }
            | Operation::UpdateNode { id, .. }
            | Operation::ReparentNode { id, .. }
            | Operation::ReorderNode { id, .. }
            | Operation::DeleteSubtree { id } => id,
            _ => return Ok(()),
        };
        if let Some((instance, _)) = id
            .split_once('/')
            .or_else(|| parent.and_then(|id| id.split_once('/')))
        {
            return Err(StorageError::new(
                "INSTANCE_CHILD_READONLY",
                &format!("Virtual children are read-only; update_node {instance} props.overrides"),
            ));
        }
        Ok(())
    }
    pub(super) fn component_delete_guard(&self, remove: &BTreeSet<String>) -> Result<()> {
        let users: Vec<_> = self
            .nodes
            .values()
            .filter(|n| {
                !remove.contains(&n.id)
                    && n.kind == NodeKind::Instance
                    && n.props["componentId"]
                        .as_str()
                        .is_some_and(|id| remove.contains(id))
            })
            .map(|n| n.id.clone())
            .collect();
        if !users.is_empty() {
            let mut error = StorageError::new(
                "COMPONENT_IN_USE",
                "Delete dependent instances before deleting their component",
            );
            error.node_context().instance_ids = Some(users.iter().take(20).cloned().collect());
            error.node_context().count = Some(users.len());
            return Err(error);
        }
        Ok(())
    }
}
