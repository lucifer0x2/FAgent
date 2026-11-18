//! Instance expansion over a validation [`Lookup`], node for node the expansion
//! `Candidate::validate` makes: it expands a map keyed by id, so children come in id
//! order (not sibling order).
use crate::instances::{expand_with, Children, Lookup as Expansion};
use crate::validate::Lookup;
use crate::{Node, Result};
use std::collections::BTreeMap;

struct Authored<'a>(&'a dyn Lookup);

impl Expansion for Authored<'_> {
    fn node(&self, id: &str) -> Option<&Node> {
        self.0.node(id)
    }

    fn children(&self, id: &str) -> Children<'_> {
        let mut children = self.0.children(id);
        children.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        Children::Indexed(Box::new(children.into_iter()))
    }
}

/// The expansion of `instances` (authored instances, in the order given): each root
/// followed by its virtual descendants, as `instances::expand` places them.
pub(super) fn expand(lookup: &dyn Lookup, instances: &[&Node]) -> Result<Vec<Node>> {
    expand_with(&Authored(lookup), instances)
}

/// Virtual nodes per instance root of an expansion.
pub(super) fn extras(nodes: &[Node]) -> BTreeMap<&str, usize> {
    let mut extras = BTreeMap::new();
    for node in nodes {
        if let Some((root, _)) = node.id.split_once('/') {
            *extras.entry(root).or_default() += 1;
        }
    }
    extras
}
