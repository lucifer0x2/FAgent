//! Admission of a loaded revision: full validation once, and the certificate that
//! lets later commits validate their delta only.
use super::{extras, Certificate};
use crate::validate::Candidate;
use crate::{read_scope::ReadView, Result, Snapshot};

/// Validates a loaded revision in full (`Candidate::validate`) and certifies it for
/// delta validation. Returns the first validation error; the view is served either way.
///
/// The certificate is set from the instance expansion **even when validation fails**:
/// stored data that is already invalid stays readable, and commits on top of it are
/// validated on their delta like on a valid revision (the reference validator is
/// incremental too, so both judge the batch, not the stored rows it does not touch).
/// When the expansion itself fails (a `componentId` that names no component master,
/// nesting too deep, too many expanded nodes) no certificate is set: every later
/// commit then takes the full reference path, which reports the stored error again
/// for batches that reach it. A view that already holds a certificate keeps it.
pub fn admit(view: &ReadView) -> Result<()> {
    crate::bench_phase::time("memory.validation.admit", || {
        let snapshot = view.to_snapshot();
        let checked = Candidate::from_snapshot(&snapshot).validate();
        match certify(&snapshot) {
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
    })
}
