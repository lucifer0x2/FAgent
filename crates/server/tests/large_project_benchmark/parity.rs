//! Untimed full validation of the exact Task24 scaled source fixture. Runs before
//! every cell's measurements; commits validate their own batches.
use fagent_storage::{validate::Candidate, Snapshot};

pub fn validate_fixture(snapshot: &Snapshot) {
    Candidate::from_snapshot(snapshot).validate().unwrap();
    eprintln!("Fixture validated: {} authored nodes", snapshot.nodes.len());
}
