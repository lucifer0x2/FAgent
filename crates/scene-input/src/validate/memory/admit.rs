//! Admission of a loaded revision: full validation once, and the certificate that
//! lets later commits validate their delta only.
use super::{extras, Certificate};
use crate::validate::Candidate;
use crate::{read_scope::ReadView, Result, Snapshot};

/// Validates a loaded revision in full (`Candidate::validate`) and certifies it for
/// delta validation. Returns the first validation error; the view is served either way.
///
/// The certificate is set from the instance expansion **even when validation fails**,
/// as long as the failure is local to rows a batch may never touch: stored data that is
/// already invalid stays readable, and commits on top of it are validated on their
/// delta like on a valid revision (the reference validator is incremental too, so both
/// judge the batch, not the stored rows it does not touch). No certificate is set when
/// the revision fails a check the reference repeats over the whole project on every
/// commit and the delta path does not ([`project_wide`]), or when the expansion itself
/// fails (a `componentId` that names no component master, nesting too deep, too many
/// expanded nodes): every later commit then takes the full reference path, which
/// reports the stored error again for batches that reach it. A view that already holds
/// a certificate keeps it.
pub fn admit(view: &ReadView) -> Result<()> {
    crate::bench_phase::time("memory.validation.admit", || {
        let snapshot = view.to_snapshot();
        let candidate = Candidate::from_snapshot(&snapshot);
        let checked = candidate.validate();
        let certificate = match &checked {
            Ok(()) => certify(&snapshot),
            Err(_) => project_wide(&candidate)
                .and_then(|()| certify(&snapshot))
                .map(Certificate::invalid),
        };
        match certificate {
            Ok(certificate) => {
                let _ = view.validation.set(certificate);
                checked
            }
            Err(error) => checked.and(Err(error)),
        }
    })
}

/// Whether `view` holds a validation certificate (commits on it may stay local).
pub fn certified(view: &ReadView) -> bool {
    view.validation.get().is_some()
}

/// The certificate of a snapshot: expanded instance children per instance root.
pub(super) fn certify(snapshot: &Snapshot) -> Result<Certificate> {
    let placed = crate::instances::expand(&snapshot.nodes)?;
    Ok(Certificate {
        extra: extras(placed.iter().map(|n| &n.id)),
        total: placed.len(),
        valid: true,
    })
}

/// The checks `Candidate::validate_after` runs over every row on each commit while
/// `validate_delta` runs them on the batch's closure only, or not at all (notes). On a
/// valid revision both agree; on one that fails them the reference rejects every batch
/// and the delta path would not. Tokens are checked in full by both and need no gate.
fn project_wide(candidate: &Candidate) -> Result<()> {
    candidate.validate_notes()?;
    candidate.validate_navigation()?;
    candidate.validate_animation_triggers()?;
    candidate.instance_graph()
}
