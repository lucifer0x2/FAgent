//! The component graph check (`Candidate::instance_graph`) around the masters a batch
//! changed. An edge `M -> T` exists when a component master `M` contains an instance of
//! `T`. On a valid base the graph was acyclic and at most 8 deep, so a new cycle or a
//! new too-long path runs through a master whose edges changed (a *start* master). The
//! full check walks from every master in id order; only the masters that reach a start
//! master (its users' enclosing masters, transitively) can fail. Which error that walk
//! meets first (cycle or depth) depends on the masters walked before, so on a fault the
//! graph is walked once more over every master in id order, as the full check does: a
//! rejected batch with a component cycle or depth fault walks the whole project once;
//! accepted batches never pay for that walk. The walk itself is `instance_graph`'s.
use crate::validate::instance_graph::{enclosing_masters, walk_all, Walk};
use crate::validate::Lookup;
use crate::{NodeKind, Result};
use std::collections::BTreeSet;

pub(super) fn check<'a>(after: &dyn Lookup, affected: impl Iterator<Item = &'a str>) -> Result<()> {
    let mut start = BTreeSet::new();
    for node in affected.filter_map(|id| after.node(id)) {
        if node.props.get("component").is_some() {
            start.insert(node.id.clone());
        }
        if node.kind == NodeKind::Instance {
            start.extend(enclosing_masters(after, node.parent_id.as_deref()));
        }
    }
    let mut roots = BTreeSet::new();
    let mut pending: Vec<String> = start.into_iter().collect();
    while let Some(master) = pending.pop() {
        if !roots.insert(master.clone()) {
            continue;
        }
        for user in after.component_users(&master) {
            let parent = after.node(user).and_then(|n| n.parent_id.as_deref());
            pending.extend(enclosing_masters(after, parent));
        }
    }
    let mut walk = Walk::lazy(after);
    for root in &roots {
        if let Err(fault) = walk.visit(root, &mut BTreeSet::new()) {
            return Err(full(after).err().unwrap_or(fault));
        }
    }
    Ok(())
}

/// `Candidate::instance_graph` over the whole project (every instance reachable from a
/// page). Runs only on a batch the scoped walk already rejected.
fn full(lookup: &dyn Lookup) -> Result<()> {
    let mut instances = Vec::new();
    let mut pending = lookup.pages();
    while let Some(node) = pending.pop() {
        pending.extend(lookup.children(&node.id));
        if node.kind == NodeKind::Instance {
            instances.push(node);
        }
    }
    walk_all(lookup, instances.into_iter())
}
