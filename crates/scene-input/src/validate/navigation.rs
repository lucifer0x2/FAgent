//! Batch-level navigation checks and the delete cascade. Destinations are checked once per
//! batch, after every operation, so a node may link to a root frame created later in it.
//! The per-node link check and the flow check are shared with the delta rules, which run
//! them on the nodes a batch touched over any [`Lookup`].
use super::{scope::Resolver, Candidate, Lookup};
use crate::{
    error::*,
    links::{flow_start, node_links},
    model::*,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

impl Candidate {
    /// Batch-level checks: every link destination is a root frame and flow names are unique.
    pub(super) fn validate_navigation(&self) -> Result<()> {
        let resolver = self.resolver();
        let frames = || self.root_frames();
        for n in self.nodes.values() {
            check_node_links(&resolver, n, &frames)?;
        }
        check_flows(self.root_frames())
    }

    /// Root frames in page order, then sortKey: the order that picks the default flow.
    fn root_frames(&self) -> Vec<&Node> {
        let mut frames: Vec<_> = self
            .nodes
            .values()
            .filter(|n| self.resolver().is_root_frame(&n.id))
            .map(|n| {
                let page = &self.nodes[n.parent_id.as_ref().unwrap()];
                ((page.sort_key, &page.id, n.sort_key, &n.id), n)
            })
            .collect();
        frames.sort_by(|a, b| a.0.cmp(&b.0));
        frames.into_iter().map(|(_, n)| n).collect()
    }

    /// Drops links to nodes this batch deleted and did not recreate; an emptied `links` key is
    /// removed. Sources join `changed`. A link that never pointed at a node still fails later.
    pub fn prune_links(&mut self, changed: &mut BTreeSet<String>) {
        if self.deleted.is_empty() {
            return;
        }
        let gone: BTreeSet<_> = self
            .deleted
            .iter()
            .filter(|id| !self.nodes.contains_key(*id))
            .cloned()
            .collect();
        for n in self.nodes.values_mut() {
            let Some(props) = n.props.as_object_mut() else {
                continue;
            };
            let Some(links) = props.get_mut("links").and_then(Value::as_array_mut) else {
                continue;
            };
            let before = links.len();
            links.retain(|l| l["to"].as_str().is_none_or(|to| !gone.contains(to)));
            if links.len() == before {
                continue;
            }
            if links.is_empty() {
                props.remove("links");
            }
            changed.insert(n.id.clone());
        }
    }
}

/// Root frames of `lookup` in page order, then sortKey (`Candidate::root_frames`' order).
pub(super) fn root_frames(lookup: &dyn Lookup) -> Vec<&Node> {
    let pages = lookup.pages().into_iter();
    let children = pages.flat_map(|page| lookup.children(&page.id));
    children.filter(|n| n.kind == NodeKind::Frame).collect()
}

/// Every link destination of `n` is a root frame; `frames` lists the root frames in
/// [`root_frames`] order, for the suggestion of a missing target.
pub(super) fn check_node_links<'a>(
    resolver: &Resolver<'a>,
    n: &Node,
    frames: &dyn Fn() -> Vec<&'a Node>,
) -> Result<()> {
    for (i, link) in node_links(&n.props).iter().enumerate() {
        if let Some(to) = &link.to {
            check_target(resolver, to, frames)
                .map_err(|(message, suggestion)| {
                    let field = format!("links[{i}].to");
                    let mut e = StorageError::validation(&field, &message);
                    e.node_context().suggestion = suggestion;
                    e
                })
                .map_err(|e| e.on_node(&n.id, n.kind.as_str()))?;
        }
    }
    Ok(())
}

/// Flow names are unique over `frames`, the root frames in [`root_frames`] order.
pub(super) fn check_flows<'a>(frames: impl IntoIterator<Item = &'a Node>) -> Result<()> {
    let mut flows: BTreeMap<&str, &str> = BTreeMap::new();
    for frame in frames {
        if let Some(name) = flow_start(&frame.props) {
            if let Some(holder) = flows.insert(name, &frame.id) {
                let message = format!("Flow '{name}' already starts at frame '{holder}'");
                return Err(StorageError::validation("flowStart", &message)
                    .on_node(&frame.id, frame.kind.as_str()));
            }
        }
    }
    Ok(())
}

/// Why `to` is not a root frame, with the root frame the author most likely meant.
fn check_target<'a>(
    resolver: &Resolver<'a>,
    to: &str,
    frames: &dyn Fn() -> Vec<&'a Node>,
) -> std::result::Result<(), (String, Option<String>)> {
    let Some(target) = resolver.find(to) else {
        let by_name = frames().into_iter().find(|f| {
            f.props["name"]
                .as_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(to))
        });
        let message = format!("Link target '{to}' does not exist; to takes a root frame ID");
        return Err((message, by_name.map(|f| f.id.clone())));
    };
    if resolver.is_root_frame(to) {
        return Ok(());
    }
    if target.kind != NodeKind::Frame {
        let kind = target.kind.as_str();
        return Err((
            format!("Link target '{to}' is a {kind}, not a root frame"),
            None,
        ));
    }
    let mut root = target;
    while let Some(parent) = root.parent_id.as_ref().and_then(|p| resolver.find(p)) {
        if parent.kind == NodeKind::Page {
            break;
        }
        root = parent;
    }
    let message = format!(
        "Link target '{to}' is inside frame '{}'; link to its root frame",
        root.id
    );
    Err((message, Some(root.id.clone())))
}
