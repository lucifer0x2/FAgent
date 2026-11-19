//! Instances on the affected nodes, in `Candidate::validate_instances`' order: the
//! component graph, the expansion of the affected instances (in id order), the expanded
//! node budget from [`Facts`] (per-master counters, no expansion), each affected node's
//! instance checks, then the properties of the expanded nodes under every theme.
//!
//! The expansion here serves the property checks only: an override or a theme scope
//! makes each instance's copy of a master node resolve on its own, and accepting a batch
//! must equal `Candidate::validate`, which checks every copy. It covers the closure's
//! instances, never the whole project.
use super::{affected::Affected, expand::expand, facts, graph, Facts};
use crate::bench_phase::{count, time};
use crate::validate::instance_checks::{check_expanded, check_instance_node};
use crate::validate::Lookup;
use crate::{NodeKind, Result};
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
    let instances: Vec<_> = ids()
        .filter_map(|id| after.node(id))
        .filter(|n| n.kind == NodeKind::Instance)
        .collect();
    // The affected instances expand to at most the new total. A batch that shrinks a
    // stored excess stays within the old total, so it gets that much room; the budget
    // decision itself is `Facts::update`'s, and an overflow here reports `max_nodes`.
    let room = max_nodes.max(facts.total());
    let expanded = time("delta.instances.expand", || {
        expand(after, &instances, room).map_err(|e| match e.code.as_str() {
            "INSTANCE_LIMIT" => facts::over_limit(max_nodes),
            _ => e,
        })
    })?;
    count("delta.instances.expanded", expanded.len());
    let next = time("delta.instances.budget", || {
        facts.update((before, after), ids(), max_nodes)
    })?;
    let order: Vec<String> = expanded.iter().map(|n| n.id.clone()).collect();
    let placed: BTreeMap<String, _> = expanded.into_iter().map(|n| (n.id.clone(), n)).collect();
    time("delta.instances.nodes", || -> Result<()> {
        for node in ids().filter_map(|id| after.node(id)) {
            check_instance_node(after, node, &placed, themes)?;
        }
        Ok(())
    })?;
    time("delta.instances.properties", || {
        let order = order.iter().map(|id| &placed[id]);
        check_expanded(after, &placed, order, themes)
    })?;
    Ok(next)
}
