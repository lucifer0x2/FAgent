//! The snapshot a tree or find read runs on.
//!
//! Scoped reads of the latest revision (`get_design_context` by frameId,
//! `get_subtree` by nodeId, `get_outline`/`find_nodes` by nodeId or frameId) ask
//! storage for a scoped snapshot, which holds a published whole-project node-memory
//! view when memory is current. The same readers run on it. A reader error in memory is
//! retried on the database snapshot, so memory never fails a read that path serves.
//! Project-wide outline/find, resource lists and viewport preparation use the same
//! revision-bound capability. Explicit `revision` reads keep the full database path.
use super::{find, pagination, tree, viewports, ReadRequest};
use crate::{error::Result, state::AppState};
use fagent_storage::SharedSnapshot;
use serde_json::Value;

pub(super) async fn read(state: &AppState, name: &str, req: ReadRequest) -> Result<Value> {
    let Some(target) = target(name, &req) else {
        let shared = today(state, &req.project_id).await?;
        return run(state, name, req, &shared).await;
    };
    let source = state
        .db({
            let id = req.project_id.clone();
            move |store| store.scoped_snapshot(&id, &target)
        })
        .await?;
    if !source.from_memory {
        return run(state, name, req, &source.shared).await;
    }
    let memory = run(state, name, req.clone(), &source.shared).await;
    if memory.is_ok() {
        return memory;
    }
    state.store.memory_read_failed();
    let shared = today(state, &req.project_id).await?;
    run(state, name, req, &shared).await
}

/// The same read on the database snapshot only, as the memory differential
/// compares it.
#[cfg(test)]
pub(super) async fn read_database(state: &AppState, name: &str, req: ReadRequest) -> Result<Value> {
    let shared = today(state, &req.project_id).await?;
    run(state, name, req, &shared).await
}

/// The scope ID of a read memory may serve, or `None` for the database path.
fn target(name: &str, req: &ReadRequest) -> Option<String> {
    if req.revision.is_some() {
        return None;
    }
    let id = match (name, &req.node_id, &req.frame_id) {
        ("get_design_context", None, Some(id)) | ("get_subtree", Some(id), None) => id,
        ("get_outline" | "find_nodes", Some(id), None)
        | ("get_outline" | "find_nodes", None, Some(id)) => id,
        ("get_outline" | "find_nodes", None, None) => return Some(String::new()),
        ("list_tokens" | "list_styles" | "list_assets" | "list_fonts", _, _) => {
            return Some(String::new())
        }
        _ => return None,
    };
    Some(id.clone())
}

async fn today(state: &AppState, project: &str) -> Result<SharedSnapshot> {
    let id = project.to_owned();
    state.db(move |store| store.shared_snapshot(&id)).await
}

/// One tree or find read on `shared`, the same on memory views and database snapshots.
async fn run(
    state: &AppState,
    name: &str,
    req: ReadRequest,
    shared: &SharedSnapshot,
) -> Result<Value> {
    if name.starts_with("list_") {
        return super::list(state, &shared.snapshot, name, req);
    }
    if name == "find_nodes" {
        return find::read_shared(shared, req);
    }
    let (scope, offset) = pagination::scope(&shared.snapshot, name, &req)?;
    let computed = if name == "get_design_context" {
        viewports::compute(state, shared, &req).await?
    } else {
        None
    };
    tree::read_shared(shared, name, req, scope, offset, computed)
}
