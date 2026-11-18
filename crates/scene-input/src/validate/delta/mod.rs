//! End-of-batch validation of one batch on its delta and dependency closure, over any
//! [`Lookup`] (the engine's live state): the rules `Candidate::validate` runs over the
//! whole project, restricted to what the batch can have changed.
//!
//! Precondition: the base passed full validation (or was admitted, see below) and every
//! operation already passed `Candidate::apply`'s per-operation checks, token writes
//! included (`check_token_write`), and the node cap. Then accepting or rejecting the batch
//! here equals `Candidate::validate` on the result. When several rules fail, the first
//! error reported may differ from `validate()`'s (a different rule or node runs first).
//!
//! On a base admitted although invalid, stored defects matter only when the batch's
//! closure reaches them: a batch that touches none of them is accepted.
//!
//! Order of the rules, as in `Candidate::validate`: notes, navigation, then per theme
//! styles and nodes, animation triggers, instances (component graph, expansion, expanded
//! node budget, per-node instance checks, properties of expanded nodes). Each runs under
//! a `delta.*` bench phase timer; `delta.closure.nodes` and `delta.instances.expanded`
//! count the closure and the expanded nodes.
mod affected;
mod expand;
mod facts;
mod graph;
mod instances;
mod navigation;
mod themes;

use super::{animation_triggers::check_triggers, scope::Resolver, Lookup};
use crate::bench_phase::{count, time};
use crate::Result;
pub use facts::{facts, Facts};

/// The rows one batch changed, by id, each list sorted.
#[derive(Clone, Copy)]
pub struct Batch<'a> {
    /// Node rows the batch wrote (created, updated, moved, renumbered).
    pub nodes: &'a [String],
    /// Node rows that existed before the batch and no longer exist.
    pub deleted: &'a [String],
    /// Note rows the batch wrote.
    pub notes: &'a [String],
    pub tokens: &'a [String],
    pub styles: &'a [String],
    /// The expanded-node limit: `MAX_NODES`, or the engine's own node cap.
    pub max_nodes: usize,
}

/// Validates the batch that turned `before` into `after`, given the facts of `before`;
/// returns the facts of `after`.
pub fn validate_batch(
    before: &dyn Lookup,
    after: &dyn Lookup,
    batch: &Batch<'_>,
    facts: &Facts,
) -> Result<Facts> {
    let affected = time("delta.closure", || affected::collect(before, after, batch));
    count("delta.closure.nodes", affected.nodes.len());
    time("delta.notes", || navigation::notes(after, batch))?;
    time("delta.navigation", || {
        navigation::links(before, after, batch)
    })?;
    let themes = themes::declared(after);
    time("delta.themes", || themes::check(after, &affected, &themes))?;
    time("delta.triggers", || -> Result<()> {
        let resolver = Resolver {
            lookup: after,
            overlay: None,
            theme: "default",
        };
        for node in affected.nodes.iter().filter_map(|id| after.node(id)) {
            check_triggers(&resolver, node)?;
        }
        Ok(())
    })?;
    instances::check(before, after, &affected, &themes, (facts, batch.max_nodes))
}
