//! Validation of an exact staged revision. A bounded candidate is deliberately
//! never treated as a complete project. The certificate is attached to one
//! immutable ReadView; structure-only loads cannot grant incremental trust.
mod admit;
mod classify;
mod closure;
#[cfg(test)]
mod tests;

use super::{scope::Resolver, Candidate, ValidationStats, ValidationWork};
use crate::{
    read_scope::{persistent::Map, ReadView},
    NodeKind, Operation, Result, StorageError, MAX_NODES,
};
use std::collections::{BTreeMap, BTreeSet};

pub use admit::{admit, certified};

#[derive(Clone)]
pub(crate) struct Certificate {
    extra: Map<usize>,
    total: usize,
    /// False for a revision [`admit`] certified although it failed validation (and
    /// for the revisions committed on top of it): it may still hold stored errors.
    valid: bool,
}
impl Certificate {
    fn invalid(self) -> Self {
        Self {
            valid: false,
            ..self
        }
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        self.extra.estimated_bytes()
    }
}

/// Explicit classifier; false means the full reference path must run.
pub fn supports(before: &ReadView, operations: &[Operation]) -> bool {
    classify::supports(before, operations)
}

/// Validate only the affected authored/instance closure, using the exact delta's
/// changed IDs. Returns None if `before` holds no certificate and is not fully valid,
/// or replay disagrees with staged rows; callers must use the full reference path.
/// A revision admitted with [`admit`] holds a certificate even when it is invalid.
/// Both views must belong to the same builtin project/incarnation write boundary.
pub fn validate_delta(
    before: &ReadView,
    after: &ReadView,
    operations: &[Operation],
    changed: &[String],
) -> Result<Option<ValidationWork>> {
    if !supports(before, operations) {
        return Ok(None);
    }
    if before.validation.get().is_none() {
        let admission = crate::bench_phase::time("memory.validation.admission", || {
            let snapshot = before.to_snapshot();
            if Candidate::from_snapshot(&snapshot).validate().is_err() {
                return Ok(None);
            }
            admit::certify(&snapshot).map(Some)
        })?;
        let Some(certificate) = admission else {
            return Ok(None);
        };
        let _ = before.validation.set(certificate);
    }
    let certificate = before.validation.get().expect("validated admission");
    // A delete that cascades to notes makes the reference validate the whole project
    // (`resources_changed`), which reports any stored error again.
    if !certificate.valid && changed.iter().any(|id| before.note(id).is_some()) {
        return Ok(None);
    }
    let affected = closure::affected(before, after, operations, changed);
    let support = closure::support(before, after, &affected);
    // Prefix replay preserves Candidate::apply's validation, public error fields
    // and operation index (including invalid intermediate props repaired later).
    let mut snapshot = before.metadata().as_ref().clone();
    snapshot.nodes = support
        .iter()
        .filter_map(|id| before.node(id).cloned())
        .collect();
    let notes: BTreeMap<_, _> = support
        .iter()
        .flat_map(|id| before.notes_on(id))
        .map(|n| (n.id.clone(), n.clone()))
        .collect();
    snapshot.notes = notes.into_values().collect();
    let mut candidate = Candidate::from_snapshot(&snapshot);
    let mut replayed = BTreeSet::new();
    for (index, op) in operations.iter().enumerate() {
        candidate
            .apply(op, &mut replayed)
            .map_err(|e| e.at(index))?;
    }
    candidate.prune_links(&mut replayed);
    if replayed.iter().cloned().collect::<Vec<_>>() != changed
        || changed.iter().any(|id| {
            candidate.nodes.get(id) != after.node(id) || candidate.notes.get(id) != after.note(id)
        })
    {
        return Ok(None);
    }
    if after.len() > MAX_NODES {
        return Err(StorageError::validation(
            "nodes",
            &format!("Project exceeds {MAX_NODES} authored nodes"),
        ));
    }
    // Notes/resources are unchanged except safe deletion cascades. Navigation
    // changes/global flow ordering are excluded by the classifier.
    candidate.validate_navigation()?;
    let themes = candidate.validate_tokens()?;
    for theme in &themes {
        let resolver = Resolver {
            lookup: &candidate,
            overlay: None,
            theme,
        };
        for id in &affected {
            if let Some(node) = candidate.nodes.get(id) {
                resolver.node(node)?;
            }
        }
    }
    candidate.validate_animation_triggers()?;
    let mut next = certificate.clone();
    next.total = next.total - before.len() + after.len();
    for id in &affected {
        next.total -= next.extra.get(id).copied().unwrap_or(0);
        next.extra.remove(id);
    }
    let expanded = candidate.validate_instances_bounded(Some(&affected), &themes, |placed| {
        let weights = extras(placed.keys());
        for id in &affected {
            if after.node(id).is_some_and(|n| n.kind == NodeKind::Instance) {
                let extra = weights.get(id).copied().unwrap_or(0);
                next.total = next.total.saturating_add(extra);
                next.extra.insert(id.clone(), extra);
            }
        }
        if next.total > MAX_NODES {
            return Err(StorageError::new(
                "INSTANCE_LIMIT",
                &format!("Expanded project exceeds {MAX_NODES} nodes"),
            ));
        }
        Ok(())
    })?;
    if next.total > MAX_NODES {
        return Err(StorageError::new(
            "INSTANCE_LIMIT",
            &format!("Expanded project exceeds {MAX_NODES} nodes"),
        ));
    }
    let stats = ValidationStats {
        authored_nodes: after.len(),
        affected_nodes: affected
            .iter()
            .filter(|id| after.node(id).is_some())
            .count(),
        expanded_nodes: expanded
            .as_ref()
            .map_or(candidate.nodes.len(), BTreeMap::len),
        affected_virtual_nodes: expanded.as_ref().map_or(0, |nodes| {
            nodes
                .keys()
                .filter(|id| {
                    id.split_once('/')
                        .is_some_and(|(root, _)| affected.contains(root))
                })
                .count()
        }),
        full_fallback: false,
    };
    let _ = after.validation.set(next);
    Ok(Some(ValidationWork {
        affected_ids: affected,
        expanded,
        stats,
    }))
}

fn extras<'a>(ids: impl Iterator<Item = &'a String>) -> Map<usize> {
    let mut extras = Map::default();
    for id in ids {
        if let Some((root, _)) = id.split_once('/') {
            extras.insert(root.into(), extras.get(root).copied().unwrap_or(0) + 1);
        }
    }
    extras
}
