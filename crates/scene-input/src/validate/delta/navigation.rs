//! Notes and navigation on the batch's delta: the note count and the notes the batch
//! wrote; the links of written nodes, and of the nodes linking to a node the batch
//! created, deleted or moved (whether a node is a root frame depends on its kind and
//! its parent's kind only); flow names when a written node is a root frame starting a
//! flow.
use super::Batch;
use crate::links::flow_start;
use crate::validate::navigation::{check_flows, check_node_links, root_frames};
use crate::validate::notes::{check_note, too_many_notes};
use crate::validate::{scope::Resolver, Lookup};
use crate::{Node, Result, MAX_NOTES};
use std::collections::BTreeSet;

pub(super) fn notes(after: &dyn Lookup, batch: &Batch<'_>) -> Result<()> {
    if after.note_count() > MAX_NOTES {
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
    let placed = |id: &&String| match (before.node(id), after.node(id)) {
        (Some(old), Some(new)) => (&old.kind, &old.parent_id) != (&new.kind, &new.parent_id),
        _ => true,
    };
    for id in batch.nodes.iter().filter(placed).chain(batch.deleted) {
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
    let flows = batch
        .nodes
        .iter()
        .filter_map(|id| after.node(id))
        .any(|n| flow_start(&n.props).is_some() && resolver.is_root_frame(&n.id));
    if flows {
        check_flows(root_frames(after))?;
    }
    Ok(())
}
