//! Structural write errors, shared by `Candidate` and `fagent-node-memory` so both
//! report the same text. Each names the node (also as `nodeId`), the field and the
//! current value an agent needs to fix its batch without a re-read (Task28 I3).
use crate::StorageError;

fn on(mut error: StorageError, id: &str) -> StorageError {
    error.node_context().node_id = Some(id.into());
    error
}

pub fn missing_node(id: &str) -> StorageError {
    let message = format!("Node `{id}` does not exist (deleted or never created)");
    on(StorageError::validation("id", &message), id)
}

pub fn duplicate_node(id: &str) -> StorageError {
    let message = format!("Node ID `{id}` already exists; choose another ID or update it");
    on(StorageError::validation("id", &message), id)
}

pub fn missing_parent(id: &str, parent: &str) -> StorageError {
    let message = format!(
        "Parent `{parent}` of `{id}` does not exist (deleted or never created); forward references are not supported"
    );
    on(StorageError::validation("parentId", &message), id)
}

pub fn not_container(id: &str, parent: &str, parent_kind: &str) -> StorageError {
    let message = format!(
        "Parent `{parent}` of `{id}` is a {parent_kind}; parent must be a page, frame or group"
    );
    on(StorageError::validation("parentId", &message), id)
}

pub fn cycle(id: &str, parent: &str) -> StorageError {
    let message = format!("Tree cycle detected: `{parent}` is `{id}` or one of its descendants");
    on(StorageError::validation("parentId", &message), id)
}

/// `siblings` counts the other children of `parent` (`None`: the project's pages).
pub fn index_out_of_bounds(
    id: &str,
    index: usize,
    parent: Option<&str>,
    siblings: usize,
) -> StorageError {
    let under = match parent {
        Some(parent) => format!("parent `{parent}`"),
        None => "the project's pages".into(),
    };
    let message = format!(
        "Sibling index {index} is out of bounds: {under} has {siblings} other children (use 0–{siblings})"
    );
    on(StorageError::validation("index", &message), id)
}
