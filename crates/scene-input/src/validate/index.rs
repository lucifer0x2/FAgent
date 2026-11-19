//! Borrowed authored-tree index over a shared topology. No resource/scene cloning.
use super::NodeTopology;
use crate::Node;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Index over exactly one immutable authored node set. Rebuild after mutation.
pub struct NodeIndex<'a> {
    nodes: BTreeMap<&'a str, &'a Node>,
    topology: Arc<NodeTopology>,
}

impl<'a> NodeIndex<'a> {
    pub fn new(nodes: impl IntoIterator<Item = &'a Node>) -> Self {
        let nodes: Vec<_> = nodes.into_iter().collect();
        let topology = Arc::new(NodeTopology::new(nodes.iter().copied()));
        Self::with_topology(nodes, topology)
    }

    pub fn with_topology(
        nodes: impl IntoIterator<Item = &'a Node>,
        topology: Arc<NodeTopology>,
    ) -> Self {
        Self {
            nodes: nodes
                .into_iter()
                .map(|node| (node.id.as_str(), node))
                .collect(),
            topology,
        }
    }

    pub fn node(&self, id: &str) -> Option<&'a Node> {
        self.nodes.get(id).copied()
    }

    pub fn children(&self, id: &str) -> impl Iterator<Item = &'a str> + '_ {
        self.topology
            .children(id)
            .filter_map(|id| self.nodes.get(id).map(|node| node.id.as_str()))
    }
}
