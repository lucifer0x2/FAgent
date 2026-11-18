//! Read-only access to one candidate revision, so validation can run over any store.
use super::tokens::{alias_targets, theme_names};
use super::Candidate;
use crate::model::*;
use crate::read_scope::references_of;
use serde_json::Value;
use std::collections::BTreeSet;

#[cfg(test)]
#[path = "lookup_tests.rs"]
mod tests;

/// The keys [`Lookup::referencing`] finds a node under: every key of
/// [`references_of`] (`token:`, `style:`, `font:`, `asset:`, `component:`, `link:`)
/// plus `theme:<name>` for the node's own string `props.theme`.
pub fn reference_keys(props: &Value) -> BTreeSet<String> {
    let mut keys = references_of(props);
    if let Some(theme) = props.get("theme").and_then(Value::as_str) {
        keys.insert(format!("theme:{theme}"));
    }
    keys
}

/// Read-only view of the authored entities validation consults.
///
/// `Candidate` is the reference implementation; other engines may answer the same
/// questions from indexes. Iterators and lists follow the orders documented per method.
/// Token-derived answers read envelopes leniently ([`theme_names`], [`alias_targets`]),
/// so they are defined on any token set and exact on a valid one.
pub trait Lookup {
    fn node(&self, id: &str) -> Option<&Node>;
    fn node_count(&self) -> usize;
    /// Pages (nodes without a parent) sorted by `(sort_key, id)`.
    fn pages(&self) -> Vec<&Node>;
    /// Direct children sorted by `(sort_key, id)`.
    fn children(&self, id: &str) -> Vec<&Node>;
    /// Ids of the nodes whose props name `key` (see [`reference_keys`]), sorted.
    fn referencing(&self, key: &str) -> Vec<&str>;
    fn token(&self, id: &str) -> Option<&Token>;
    /// Every token in id order.
    fn tokens(&self) -> Box<dyn Iterator<Item = &Token> + '_>;
    fn token_count(&self) -> usize;
    /// Whether any token declares the named theme branch.
    fn theme_declared(&self, name: &str) -> bool {
        self.tokens().any(|t| theme_names(t).any(|n| n == name))
    }
    /// Distinct theme names declared over all tokens, `default` excluded.
    fn named_themes(&self) -> usize {
        self.declared_themes().len()
    }
    /// The theme names [`Self::named_themes`] counts, sorted.
    fn declared_themes(&self) -> Vec<&str> {
        let names = self.tokens().flat_map(theme_names);
        let names: BTreeSet<&str> = names.filter(|n| *n != "default").collect();
        names.into_iter().collect()
    }
    /// Ids of the tokens with an `aliasId` branch naming `token`, sorted.
    fn aliasers(&self, token: &str) -> Vec<&str> {
        let aliasing = self.tokens().filter(|t| alias_targets(t).contains(token));
        aliasing.map(|t| t.id.as_str()).collect()
    }
    fn style(&self, id: &str) -> Option<&Style>;
    /// Every style in id order.
    fn styles(&self) -> Box<dyn Iterator<Item = &Style> + '_>;
    fn font(&self, id: &str) -> Option<&Font>;
    /// Every font in id order.
    fn fonts(&self) -> Box<dyn Iterator<Item = &Font> + '_>;
    fn asset(&self, id: &str) -> Option<&Asset>;
    fn note(&self, id: &str) -> Option<&Note>;
    fn note_count(&self) -> usize;
    /// Notes attached to `node`, in note id order.
    fn notes_on(&self, node: &str) -> Vec<&Note>;
    /// Instances whose `props.componentId` is `master`, sorted by id.
    fn component_users(&self, master: &str) -> Vec<&str>;
    /// Nodes with a `props.links[].to` equal to `target`, sorted by id.
    fn incoming_links(&self, target: &str) -> Vec<&str>;
}

fn sibling_order(nodes: &mut [&Node]) {
    nodes.sort_by(|a, b| (a.sort_key, &a.id).cmp(&(b.sort_key, &b.id)));
}

/// The oracle implementation: plain scans over the candidate maps.
impl Lookup for Candidate {
    fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }
    fn node_count(&self) -> usize {
        self.nodes.len()
    }
    fn pages(&self) -> Vec<&Node> {
        let mut pages: Vec<&Node> = self
            .nodes
            .values()
            .filter(|n| n.parent_id.is_none())
            .collect();
        sibling_order(&mut pages);
        pages
    }
    fn children(&self, id: &str) -> Vec<&Node> {
        let mut children: Vec<&Node> = self
            .nodes
            .values()
            .filter(|n| n.parent_id.as_deref() == Some(id))
            .collect();
        sibling_order(&mut children);
        children
    }
    fn referencing(&self, key: &str) -> Vec<&str> {
        self.nodes
            .values()
            .filter(|n| reference_keys(&n.props).contains(key))
            .map(|n| n.id.as_str())
            .collect()
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
    fn styles(&self) -> Box<dyn Iterator<Item = &Style> + '_> {
        Box::new(self.styles.values())
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
    fn note_count(&self) -> usize {
        self.notes.len()
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
