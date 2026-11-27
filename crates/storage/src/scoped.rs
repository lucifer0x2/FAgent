//! Page guards use durable commit footprints, never client-supplied dependencies.
use crate::*;
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod dependencies;
pub(crate) mod dependency_index;
mod history;
pub(crate) mod index;
mod navigation;
mod notes;
#[cfg(test)]
pub(crate) mod parity;
#[cfg(test)]
mod reference_history;
pub(crate) use history::coverage;
pub(crate) use history::envelope_with_scopes;
pub(crate) use history::{envelope, operations, request_value, scrub};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedBatchRequest {
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub strict: bool,
    pub idempotency_key: String,
    pub page_ids: Vec<String>,
    pub operations: Vec<Operation>,
}
impl ScopedBatchRequest {
    pub(crate) fn split(self) -> (BatchRequest, Vec<String>) {
        (
            BatchRequest {
                project_id: self.project_id,
                expected_revision: self.expected_revision,
                strict: self.strict,
                idempotency_key: self.idempotency_key,
                operations: self.operations,
            },
            self.page_ids,
        )
    }
}

/// A snapshot and its guard always come from the same storage read.
pub fn edit_context(snapshot: Snapshot, page_ids: Vec<String>) -> Result<Value> {
    let indexes = RevisionIndexes::new(&snapshot.nodes);
    edit_context_indexed(&snapshot, &indexes, page_ids)
}

pub(crate) fn edit_context_indexed(
    snapshot: &Snapshot,
    indexes: &RevisionIndexes,
    page_ids: Vec<String>,
) -> Result<Value> {
    index::PageIndex::validate_page_ids(&page_ids)?;
    let index = indexes.dependencies(|| {
        dependency_index::DependencyIndex::with_snapshot_tree(
            snapshot,
            indexes.pages.clone(),
            Some(&indexes.references),
        )
    })?;
    index.tree.validate_pages(&page_ids)?;
    let dependencies = index.closure(&page_ids)?;
    let notes = snapshot.notes.len();
    Ok(edit_context_response(
        snapshot,
        page_ids,
        &dependencies,
        notes,
    ))
}

/// Shared wire formatter for the legacy and persistent dependency adapters. `note_count`
/// is the project's note count at this revision; `noteLimit` is the configured cap.
pub(crate) fn edit_context_response(
    snapshot: &Snapshot,
    page_ids: Vec<String>,
    dependencies: &BTreeSet<String>,
    note_count: usize,
) -> Value {
    json!({"projectId":snapshot.project_id,"expectedRevision":snapshot.revision,
        "pageIds":page_ids,"dependencyCount":dependencies.len(),
        "dependencies":dependencies.iter().take(100).collect::<Vec<_>>(),
        "dependenciesTruncated":dependencies.len()>100,
        "noteCount":note_count,"noteLimit":note_limits().notes})
}
