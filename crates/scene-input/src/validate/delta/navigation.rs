//! Notes and navigation on the batch's delta: the note count when the batch raises it,
//! and the notes the batch wrote; the links of written nodes, and of the nodes linking
//! to a node the batch created, deleted, moved or turned into (or out of) a root frame
//! (whether a node is a root frame depends on its kind and its parent's kind only); flow
//! names of the root frames the batch changed, through the `flow:` reference index.
use super::Batch;
use crate::links::flow_start;
use crate::validate::navigation::{check_flows, check_node_links, root_frames};
use crate::validate::notes::{check_note, too_many_notes};
use crate::validate::{scope::Resolver, Lookup};
use crate::{Node, NodeKind, Result, MAX_NOTES};
use std::collections::BTreeSet;

/// The note limit fails only when the batch raises the count above it (or further
/// above a stored excess); a batch that leaves the count as it was does not reach it.
pub(super) fn notes(before: &dyn Lookup, after: &dyn Lookup, batch: &Batch<'_>) -> Result<()> {
    let count = after.note_count();
    if count > MAX_NOTES && count > before.note_count() {
        return Err(too_many_notes());
    }
    for note in batch.notes.iter().filter_map(|id| after.note(id)) {
        check_note(after, note)?;
    }
    Ok(())
}

pub(super) fn links(before: &dyn Lookup, after: &dyn Lookup, batch: &Batch<'_>) -> Result<()> {
    // Nodes whose links may have changed (a renumbered sibling keeps its props).
    let written = batch.nodes.iter().filter_map(|id| after.node(id));
    let linking = |n: &&Node| {
        n.props.get("links").is_some() && before.node(&n.id).is_none_or(|old| old.props != n.props)
    };
    let mut sources: BTreeSet<&str> = written.filter(linking).map(|n| n.id.as_str()).collect();
    for id in batch.nodes.iter().filter(|id| placed(before, after, id)) {
        sources.extend(after.incoming_links(id));
    }
    for id in batch.deleted {
        sources.extend(after.incoming_links(id));
    }
    let resolver = Resolver {
        lookup: after,
        overlay: None,
        theme: "default",
    };
    let frames = || root_frames(after);
    for node in sources.into_iter().filter_map(|id| after.node(id)) {
        check_node_links(&resolver, node, &frames)?;
    }
    flows(before, after, batch)
}

/// Whether the batch created `id`, or changed its kind, its parent or whether it is a
/// root frame (a node recreated under a parent of another kind keeps its parent id).
fn placed(before: &dyn Lookup, after: &dyn Lookup, id: &str) -> bool {
    match (before.node(id), after.node(id)) {
        (Some(old), Some(new)) => {
            (&old.kind, &old.parent_id) != (&new.kind, &new.parent_id)
                || is_root_frame(before, old) != is_root_frame(after, new)
        }
        _ => true,
    }
}

/// A frame whose parent is a page.
fn is_root_frame(view: &dyn Lookup, node: &Node) -> bool {
    node.kind == NodeKind::Frame
        && node
            .parent_id
            .as_deref()
            .and_then(|id| view.node(id))
            .is_some_and(|parent| parent.kind == NodeKind::Page)
}

/// Flow names of the root frames the batch created, placed or whose props changed: each
/// name is checked over the root frames starting it (the `flow:` index), in
/// [`root_frames`] order, as `check_flows` meets them.
fn flows(before: &dyn Lookup, after: &dyn Lookup, batch: &Batch<'_>) -> Result<()> {
    let mut names = BTreeSet::new();
    for node in batch.nodes.iter().filter_map(|id| after.node(id)) {
        let Some(name) = flow_start(&node.props) else {
            continue;
        };
        let changed = before
            .node(&node.id)
            .is_none_or(|old| old.props != node.props);
        if (changed || placed(before, after, &node.id)) && is_root_frame(after, node) {
            names.insert(name);
        }
    }
    for name in names {
        let starting = after.referencing(&format!("flow:{name}"));
        let mut frames: Vec<(_, &Node)> = starting
            .into_iter()
            .filter_map(|id| after.node(id))
            .filter(|n| is_root_frame(after, n))
            .filter_map(|n| {
                let page = after.node(n.parent_id.as_deref()?)?;
                Some(((page.sort_key, &page.id, n.sort_key, &n.id), n))
            })
            .collect();
        if frames.len() > 1 {
            frames.sort_by(|a, b| a.0.cmp(&b.0));
            check_flows(frames.into_iter().map(|(_, n)| n))?;
        }
    }
    Ok(())
}
