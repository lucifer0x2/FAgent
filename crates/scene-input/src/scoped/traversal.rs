//! Reader ordering and navigation over the published topology. Legacy vectors
//! keep their stable source-order tie break; persistent views use storage order.
use super::ScopedSnapshot;
use crate::Node;

impl<'a> ScopedSnapshot<'a> {
    pub fn is_persistent(&self) -> bool {
        self.view.is_some()
    }

    pub fn child_count(&self, id: &str) -> usize {
        match self.view {
            Some(view) => view.source.child_count(id),
            None => self.index.children.get(id).map_or(0, Vec::len),
        }
    }

    pub fn ordered_roots(&self) -> Box<dyn Iterator<Item = &'a Node> + '_> {
        if self.view.is_some() {
            return self.roots();
        }
        let mut nodes: Vec<_> = self.roots().collect();
        nodes.sort_by_key(|n| n.sort_key);
        Box::new(nodes.into_iter())
    }

    pub fn ordered_children(&self, id: &str) -> Box<dyn Iterator<Item = &'a Node> + '_> {
        if self.view.is_some() {
            return self.children(id);
        }
        let mut nodes: Vec<_> = self.children(id).collect();
        nodes.sort_by_key(|n| n.sort_key);
        Box::new(nodes.into_iter())
    }

    /// Incoming sources in project preorder, sorting only the requested edges.
    pub fn incoming_nodes(&self, frame: &str) -> Vec<&'a Node> {
        let Some(view) = self.view else {
            return Vec::new();
        };
        let mut nodes: Vec<_> = view
            .source
            .linking(frame)
            .into_iter()
            .filter_map(|id| view.node(id))
            .collect();
        let path = |node: &'a Node| {
            let mut path = Vec::new();
            let mut current = Some(node);
            for _ in 0..=128 {
                let Some(node) = current else {
                    break;
                };
                path.push((node.sort_key, node.id.as_str()));
                current = node.parent_id.as_deref().and_then(|id| view.node(id));
            }
            path.reverse();
            path
        };
        nodes.sort_by_cached_key(|node| path(node));
        nodes
    }
}
