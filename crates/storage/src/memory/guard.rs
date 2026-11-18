//! Strict page guards and last-writer-wins results over a prepared batch's views.
//!
//! [`check`] is `scoped::check` on the engine's `before`/`after` views instead of a
//! database snapshot, a `Candidate` and `BatchScope` page indexes: the same checks
//! in the same order, the same errors. The footprint is the engine's
//! (`Prepared::footprint`), the dependency closures are the views' own. `guard_tests`
//! compares both for every operation class.
use crate::scoped::{coverage, index::PageIndex};
use crate::{concurrent, BatchRequest, BatchResult, NodeKind, Operation, Result, StorageError};
use fagent_node_memory::Prepared;
use fagent_scene_input::read_scope::ReadView;
use serde_json::Value;
use std::collections::BTreeSet;

/// Strict mode with `pageIds` only, which requires an expected revision.
pub(crate) fn check(
    prepared: &Prepared<'_>,
    request: &BatchRequest,
    pages: &[String],
    commits: &[(u64, Value)],
) -> Result<()> {
    let (before, after) = (prepared.before().as_ref(), prepared.after().as_ref());
    let revision = prepared.base_revision();
    let base = request.expected_revision.unwrap_or(revision);
    if base > revision || commits.len() > 1000 {
        return Err(StorageError::conflict(revision));
    }
    // A guarded page exists already or is appended by this batch.
    PageIndex::validate_page_ids(pages)?;
    let guarded = |page: &str| pages.iter().any(|p| p == page);
    if !pages
        .iter()
        .all(|p| is_page(before, p) || is_page(after, p))
    {
        return Err(StorageError::validation(
            "pageIds",
            "Guarded page does not exist",
        ));
    }
    notes(before, after, &request.operations, &guarded)?;
    for op in &request.operations {
        use Operation::*;
        let (id, parent, props) = match op {
            CreateNode {
                id,
                parent_id,
                props,
                ..
            } => (id, parent_id.as_deref(), Some(props)),
            UpdateNode { id, props, .. } => (id, None, Some(props)),
            ReparentNode { id, parent_id, .. } => (id, Some(parent_id.as_str()), None),
            ReorderNode { id, .. } | DeleteSubtree { id } => (id, None, None),
            CreateNote { .. } | ReplyNote { .. } | UpdateNote { .. } | DeleteNote { .. } => {
                continue
            }
            _ => {
                return Err(unsupported(
                    "Token and style writes require strict mode (omit pageIds)",
                ))
            }
        };
        if props.is_some_and(|p| p.get("flowStart").is_some()) {
            return Err(unsupported("Global flowStart changes require strict mode"));
        }
        for (view, target) in [(before, id.as_str()), (after, id.as_str())]
            .into_iter()
            .chain(parent.map(|p| (after, p)))
        {
            if view.page(target).is_some_and(|page| !guarded(page)) {
                return Err(unsupported(
                    "Operation source and destination pages must be guarded",
                ));
            }
        }
        match op {
            CreateNode { index: Some(_), .. } if is_root(after, id) => {
                return Err(unsupported(
                    "Append root frames without an explicit index in scoped mode",
                ));
            }
            DeleteSubtree { .. } | ReparentNode { .. } | ReorderNode { .. }
                if is_root(before, id) || is_root(after, id) =>
            {
                return Err(unsupported(
                    "Root-frame deletion, reparent or reorder requires strict mode",
                ));
            }
            _ => {}
        }
    }
    let footprint = prepared.footprint();
    if footprint.contains("*") {
        return Err(unsupported("Changing the default page or first root frame, page order, page deletion or page properties requires strict mode"));
    }
    for key in footprint {
        if let Some(page) = key.strip_prefix("page:") {
            if !guarded(page) {
                return Err(unsupported("Every affected page, including link cleanup, note anchors and source/destination pages, must be guarded"));
            }
        } else if !key.starts_with("node:") && !key.starts_with("note:") {
            return Err(unsupported("Scoped shared-resource writes are unsupported"));
        }
    }
    // At the current revision there is no concurrent history to intersect.
    let mut guard = BTreeSet::new();
    if base != revision {
        // Pages appended by this batch have no old closure; their new closure
        // still guards the page key against concurrent create/delete.
        let old_pages: Vec<_> = pages
            .iter()
            .filter(|p| is_page(before, p))
            .cloned()
            .collect();
        if !old_pages.is_empty() {
            guard = before.dependency_closure(&old_pages)?;
        }
        // New references need protection too; a client cannot attach a resource
        // changed since its read even if the old page did not reference it.
        guard.extend(after.dependency_closure(pages)?);
    }
    let mut expected = base + 1;
    for (commit, value) in commits {
        if *commit != expected || value["scopeVersion"] != 1 {
            return Err(StorageError::conflict(revision));
        }
        let project = &before.metadata().project_id;
        let scopes =
            coverage(value, project, commit - 1).map_err(|_| StorageError::conflict(revision))?;
        if scopes.iter().any(|key| key == "*" || guard.contains(key)) {
            return Err(StorageError::conflict(revision));
        }
        expected += 1;
    }
    if expected != revision + 1 {
        return Err(StorageError::conflict(revision));
    }
    Ok(())
}

/// The commit's result, with the pairs it overwrote when it is based on an older
/// revision. `commits` are those after the request's base; `operations` the
/// canonical request's operations.
pub(crate) fn result(
    prepared: &Prepared<'_>,
    request: &BatchRequest,
    commits: &[(u64, Value)],
    operations: &Value,
) -> BatchResult {
    let (before, after) = (prepared.before(), prepared.after());
    let current = prepared.base_revision();
    let mut result = BatchResult {
        project_id: before.metadata().project_id.clone(),
        committed_revision: prepared.revision(),
        changed_ids: prepared.changed_ids().to_vec(),
        render_status: "pending".into(),
        ..Default::default()
    };
    let Some(expected) = request.expected_revision.filter(|base| *base < current) else {
        return result;
    };
    let meta = after.metadata();
    let resources: BTreeSet<&str> = meta
        .tokens
        .iter()
        .map(|t| t.id.as_str())
        .chain(meta.styles.iter().map(|s| s.id.as_str()))
        .collect();
    let ours = concurrent::batch_writes(operations, &result.changed_ids, |id| {
        after.node(id).is_some() || after.note(id).is_some() || resources.contains(id)
    });
    concurrent::report(&mut result, expected, current, commits, &ours);
    result
}

/// Note writes need their current, new and requested anchors on guarded pages.
fn notes(
    before: &ReadView,
    after: &ReadView,
    operations: &[Operation],
    guarded: &impl Fn(&str) -> bool,
) -> Result<()> {
    for op in operations {
        let (id, anchor) = match op {
            Operation::CreateNote { id, node_id, .. } => (id, Some(node_id)),
            Operation::UpdateNote { id, node_id, .. } => (id, node_id.as_ref()),
            Operation::ReplyNote { id, .. } | Operation::DeleteNote { id } => (id, None),
            _ => continue,
        };
        // A requested anchor may be deleted later in the batch; its old page counts.
        let anchors = [
            before.note(id).map(|n| before.page(&n.node_id)),
            after.note(id).map(|n| after.page(&n.node_id)),
            anchor.map(|node| after.page(node).or_else(|| before.page(node))),
        ];
        for page in anchors.into_iter().flatten() {
            if !page.is_some_and(guarded) {
                return Err(unsupported(
                    "Note anchors (current and new) must lie on guarded pages",
                ));
            }
        }
    }
    Ok(())
}

fn is_page(view: &ReadView, id: &str) -> bool {
    view.node(id).is_some_and(|n| n.kind == NodeKind::Page)
}

/// A frame directly on a page.
fn is_root(view: &ReadView, id: &str) -> bool {
    view.node(id).is_some_and(|n| {
        n.kind == NodeKind::Frame
            && n.parent_id
                .as_deref()
                .is_some_and(|parent| is_page(view, parent))
    })
}

fn unsupported(message: &str) -> StorageError {
    StorageError::new("SCOPED_OPERATION_UNSUPPORTED", message)
}
