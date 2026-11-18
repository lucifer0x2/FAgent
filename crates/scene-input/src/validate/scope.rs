//! Borrowed resolution context: node scopes never clone the candidate maps.
use super::{Candidate, Lookup};
use crate::{Node, Result, StorageError, Style};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub(super) struct Resolver<'a> {
    pub lookup: &'a dyn Lookup,
    /// A full replacement node set (an instance expansion); `None` reads `lookup`.
    pub overlay: Option<&'a BTreeMap<String, Node>>,
    pub theme: &'a str,
}

/// Validate one node exactly as each node write in `Candidate::apply` does, at the
/// default theme: ID, structural parent/cycle/depth, then resolved properties.
pub fn check_node(lookup: &dyn Lookup, node: &Node) -> Result<()> {
    check_node_at(lookup, node, "default")
}

/// Validate one style exactly as `Candidate::apply` does for `upsert_style`.
pub fn check_style(lookup: &dyn Lookup, style: &Style) -> Result<()> {
    check_style_at(lookup, style, "default")
}

pub(super) fn check_node_at(lookup: &dyn Lookup, node: &Node, theme: &str) -> Result<()> {
    Resolver {
        lookup,
        overlay: None,
        theme,
    }
    .node(node)
}

pub(super) fn check_style_at(lookup: &dyn Lookup, style: &Style, theme: &str) -> Result<()> {
    Resolver {
        lookup,
        overlay: None,
        theme,
    }
    .style(style)
}

impl Candidate {
    pub(super) fn resolver(&self) -> Resolver<'_> {
        Resolver {
            lookup: self,
            overlay: None,
            theme: &self.theme,
        }
    }
    pub(super) fn effective_theme<'a>(&'a self, node: &'a Node) -> Result<&'a str> {
        self.resolver().effective_theme(node)
    }
    pub fn resolve_props(&self, node: &Node) -> Result<Value> {
        self.resolver().resolve_scoped_props(node)
    }
    pub fn resolve_value(&self, value: &Value, expected: &str) -> Result<Value> {
        self.resolver().resolve_value(value, expected)
    }
    /// `check_node` at the candidate's selected theme (always `default` while applying).
    pub(super) fn node(&self, node: &Node) -> Result<()> {
        check_node_at(self, node, &self.theme)
    }
    /// `check_style` at the candidate's selected theme (always `default` while applying).
    pub(super) fn style(&self, style: &Style) -> Result<()> {
        check_style_at(self, style, &self.theme)
    }
}
impl<'context> Resolver<'context> {
    /// The node at `id` in the overlay when present, else in the lookup.
    pub(super) fn find(&self, id: &str) -> Option<&'context Node> {
        match self.overlay {
            Some(nodes) => nodes.get(id),
            None => {
                let lookup: &'context dyn Lookup = self.lookup;
                lookup.node(id)
            }
        }
    }
    pub(super) fn node_count(&self) -> usize {
        self.overlay
            .map_or_else(|| self.lookup.node_count(), BTreeMap::len)
    }
    pub(super) fn effective_theme<'a>(&self, node: &'a Node) -> Result<&'a str>
    where
        'context: 'a,
    {
        let mut current = Some(node);
        // Expansion can concatenate several individually valid authored ancestor chains.
        for _ in 0..=self.node_count() {
            let Some(n) = current else { break };
            if let Some(value) = n.props.get("theme") {
                let name = value.as_str().ok_or_else(|| {
                    StorageError::validation("props.theme", "Theme must be a string")
                })?;
                if name != "default" && !self.lookup.theme_declared(name) {
                    let mut error = StorageError::new(
                        "UNKNOWN_THEME",
                        "Scoped theme is not declared by any token",
                    );
                    error.field = Some("props.theme".into());
                    return Err(error);
                }
                return Ok(name);
            }
            current = n.parent_id.as_ref().and_then(|id| self.find(id));
        }
        Ok(self.theme)
    }
    pub(super) fn resolve_scoped_props(&self, node: &Node) -> Result<Value> {
        Resolver {
            theme: self.effective_theme(node)?,
            ..*self
        }
        .resolve_props(node)
    }
    pub(super) fn ancestors(&self, id: &str) -> impl Iterator<Item = &'context Node> {
        let this = *self;
        let parent = move |node: &Node| node.parent_id.as_ref().and_then(|id| this.find(id));
        std::iter::successors(self.find(id).and_then(parent), move |node| parent(node)).take(128)
    }
    pub(super) fn is_root_frame(&self, id: &str) -> bool {
        self.find(id).is_some_and(|n| {
            n.kind == crate::NodeKind::Frame
                && n.parent_id
                    .as_ref()
                    .and_then(|id| self.find(id))
                    .is_some_and(|n| n.kind == crate::NodeKind::Page)
        })
    }
}
