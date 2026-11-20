//! Instances on the affected nodes: the component graph, the expanded node budget from
//! [`Facts`] (per-master counters, no expansion), then each affected node's instance
//! checks. An affected instance is expanded on its own, checked (keys, master, override
//! targets, then the properties of its expanded nodes under every theme) and dropped
//! before the next, so validation never holds every affected instance's virtual nodes.
//!
//! The expansion serves the property checks only: an override or a theme scope makes
//! each instance's copy of a master node resolve on its own, and accepting a batch must
//! equal `Candidate::validate`, which checks every copy. It covers the closure's
//! instances, never the whole project. Errors are the full validator's; when a batch
//! has several, the first reported may differ (task rule 6).
use super::{affected::Affected, expand::expand, facts, graph, Facts};
use crate::bench_phase::{count, time};
use crate::validate::instance_checks::{check_expanded, check_instance_node};
use crate::validate::Lookup;
use crate::{Node, NodeKind, Result};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn check(
    before: &dyn Lookup,
    after: &dyn Lookup,
    affected: &Affected,
    themes: &BTreeSet<String>,
    (facts, max_nodes): (&Facts, usize),
) -> Result<Facts> {
    let ids = || affected.nodes.iter().map(String::as_str);
    time("delta.instances.graph", || graph::check(after, ids()))?;
    let next = time("delta.instances.budget", || {
        facts.update((before, after), ids(), max_nodes)
    })?;
    // Within the budget the new total, which bounds every instance's expansion, is at
    // most the larger of the limit and the stored total (a batch may shrink an excess).
    let room = max_nodes.max(facts.total());
    let empty = BTreeMap::new();
    time("delta.instances.nodes", || -> Result<()> {
        for node in ids().filter_map(|id| after.node(id)) {
            if node.kind != NodeKind::Instance {
                check_instance_node(after, node, &empty, themes)?;
                continue;
            }
            let expanded = expand(after, &[node], room).map_err(|e| match e.code.as_str() {
                "INSTANCE_LIMIT" => facts::over_limit(max_nodes),
                _ => e,
            })?;
            count("delta.instances.expanded", expanded.len());
            check_one(after, node, expanded, themes)?;
        }
        Ok(())
    })?;
    Ok(next)
}

/// One instance's checks over its own expansion, which is dropped on return.
fn check_one(
    after: &dyn Lookup,
    node: &Node,
    expanded: Vec<Node>,
    themes: &BTreeSet<String>,
) -> Result<()> {
    let order: Vec<String> = expanded.iter().map(|n| n.id.clone()).collect();
    let placed: BTreeMap<String, Node> = expanded.into_iter().map(|n| (n.id.clone(), n)).collect();
    check_instance_node(after, node, &placed, themes)?;
    check_expanded(after, &placed, order.iter().map(|id| &placed[id]), themes)
}
