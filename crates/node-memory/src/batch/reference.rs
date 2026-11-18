//! The reference validator, run inside the engine on the batch's base view: the same
//! `Candidate` steps a commit used to run against a database-loaded snapshot, with no
//! database, cache or index involved.
use fagent_scene_input::read_scope::ReadView;
use fagent_scene_input::validate::Candidate;
use fagent_scene_input::{Operation, Result};
use std::collections::BTreeSet;

/// Applies every operation to a candidate built from `before` (errors carry the
/// operation index), prunes links, and validates the result. Returns the changed ids.
/// `before` must itself be valid: this is the commit-time validator, not an import
/// check.
pub(super) fn run(before: &ReadView, ops: &[Operation]) -> Result<Vec<String>> {
    let snapshot = before.to_snapshot();
    let mut candidate = Candidate::from_snapshot(&snapshot);
    let mut changed = BTreeSet::new();
    for (index, op) in ops.iter().enumerate() {
        candidate.apply(op, &mut changed).map_err(|e| e.at(index))?;
    }
    candidate.prune_links(&mut changed);
    candidate.validate_after(&snapshot, ops, &changed)?;
    Ok(changed.into_iter().collect())
}
