//! Read-only access to one candidate revision, so validation can run over any store.
use super::Candidate;
use crate::model::*;
use serde_json::Value;

#[cfg(test)]
#[path = "lookup_tests.rs"]
mod tests;

/// Read-only view of the authored entities validation consults.
///
/// `Candidate` is the reference implementation; other engines may answer the same
/// questions from indexes. Iterators and lists follow the orders documented per method.
pub trait Lookup {
    fn node(&self, id: &str) -> Option<&Node>;
    fn node_count(&self) -> usize;
    /// Direct children sorted by `(sort_key, id)`.
    fn children(&self, id: &str) -> Vec<&Node>;
    fn token(&self, id: &str) -> Option<&Token>;
    /// Every token in id order.
    fn tokens(&self) -> Box<dyn Iterator<Item = &Token> + '_>;
    fn token_count(&self) -> usize;
    /// Whether any token declares the named theme branch.
    fn theme_declared(&self, name: &str) -> bool {
        self.tokens().any(|t| {
            t.value
                .get("themes")
                .and_then(Value::as_object)
                .is_some_and(|themes| themes.contains_key(name))
        })
    }
    fn style(&self, id: &str) -> Option<&Style>;
    fn font(&self, id: &str) -> Option<&Font>;
    /// Every font in id order.
    fn fonts(&self) -> Box<dyn Iterator<Item = &Font> + '_>;
    fn asset(&self, id: &str) -> Option<&Asset>;
    fn note(&self, id: &str) -> Option<&Note>;
    /// Notes attached to `node`, in note id order.
    fn notes_on(&self, node: &str) -> Vec<&Note>;
    /// Instances whose `props.componentId` is `master`, sorted by id.
    fn component_users(&self, master: &str) -> Vec<&str>;
    /// Nodes with a `props.links[].to` equal to `target`, sorted by id.
    fn incoming_links(&self, target: &str) -> Vec<&str>;
}

/// The oracle implementation: plain scans over the candidate maps.
impl Lookup for Candidate {
    fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }
    fn node_count(&self) -> usize {
        self.nodes.len()
    }
    fn children(&self, id: &str) -> Vec<&Node> {
        let mut children: Vec<&Node> = self
            .nodes
            .values()
            .filter(|n| n.parent_id.as_deref() == Some(id))
            .collect();
        children.sort_by(|a, b| (a.sort_key, &a.id).cmp(&(b.sort_key, &b.id)));
        children
    }
    fn token(&self, id: &str) -> Option<&Token> {
        self.tokens.get(id)
    }
    fn tokens(&self) -> Box<dyn Iterator<Item = &Token> + '_> {
        Box::new(self.tokens.values())
    }
    fn token_count(&self) -> usize {
        self.tokens.len()
    }
    fn style(&self, id: &str) -> Option<&Style> {
        self.styles.get(id)
    }
    fn font(&self, id: &str) -> Option<&Font> {
        self.fonts.get(id)
    }
    fn fonts(&self) -> Box<dyn Iterator<Item = &Font> + '_> {
        Box::new(self.fonts.values())
    }
    fn asset(&self, id: &str) -> Option<&Asset> {
        self.assets.get(id)
    }
    fn note(&self, id: &str) -> Option<&Note> {
        self.notes.get(id)
    }
    fn notes_on(&self, node: &str) -> Vec<&Note> {
        self.notes.values().filter(|n| n.node_id == node).collect()
    }
    fn component_users(&self, master: &str) -> Vec<&str> {
        self.nodes
            .values()
            .filter(|n| n.kind == NodeKind::Instance && n.props["componentId"] == master)
            .map(|n| n.id.as_str())
            .collect()
    }
    fn incoming_links(&self, target: &str) -> Vec<&str> {
        self.nodes
            .values()
            .filter(|n| {
                n.props
                    .get("links")
                    .and_then(Value::as_array)
                    .is_some_and(|links| links.iter().any(|link| link["to"] == target))
            })
            .map(|n| n.id.as_str())
            .collect()
    }
}
