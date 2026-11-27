mod animated;
pub(crate) mod changes;
mod compact;
mod diagnostics;
mod find;
mod index;
mod instances;
mod memory;
#[cfg(test)]
mod memory_diff;
#[cfg(test)]
mod memory_verify;
mod navigation;
mod notes;
pub(crate) mod pagination;
#[cfg(test)]
mod scoped_tree_bounds_tests;
#[cfg(test)]
pub(crate) mod scoped_tree_fixture;
#[cfg(test)]
mod scoped_tree_tests;
mod states;
#[cfg(test)]
mod stored_corruption_tests;
pub(crate) mod targets;
#[cfg(test)]
mod traversal_tests;
mod tree;
mod tree_authored;
mod tree_legacy;
mod tree_navigation;
mod tree_nodes;
mod tree_resources;
#[cfg(test)]
mod tree_work;
mod viewports;
use crate::{error::Result, state::AppState};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Deserialize, Serialize, Default, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReadRequest {
    project_id: String,
    node_id: Option<String>,
    frame_id: Option<String>,
    depth: Option<usize>,
    limit: Option<usize>,
    budget: Option<usize>,
    ids: Option<Vec<String>>,
    properties: Option<Vec<String>>,
    viewport: Option<Value>,
    viewports: Option<Vec<viewports::Viewport>>,
    theme: Option<String>,
    cursor: Option<String>,
    revision: Option<u64>,
    // find_nodes only. Skipped when absent so older tools keep their cursor scope hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kinds: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name_contains: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text_contains: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent_id: Option<String>,
    // get_design_context only; false is normalized to absent so the default scope hash holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    compact: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    node_format: Option<NodeFormat>,
    // get_design_context only; "open" (the default) is normalized to absent, like compact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    notes: Option<notes::Filter>,
    // get_design_context and find_nodes; "reference" (the default) is normalized to absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    instances: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    states_mode: Option<String>,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
enum NodeFormat {
    Rows,
}

pub async fn read(state: &AppState, name: &str, args: Value) -> Result<Value> {
    let req = request(name, args)?;
    memory::read(state, name, req).await
}

/// [`read`] on the database snapshot only, never on a node-memory view.
#[cfg(test)]
pub(crate) async fn read_database(state: &AppState, name: &str, args: Value) -> Result<Value> {
    let req = request(name, args)?;
    memory::read_database(state, name, req).await
}

/// `REVISION_UNAVAILABLE` for a read, render or export of a revision other than the latest:
/// only the latest revision is kept readable, so the error names it (`currentRevision`).
pub(crate) fn revision_unavailable(requested: u64, current: u64) -> crate::ApiError {
    let mut error = crate::ApiError::new(
        "REVISION_UNAVAILABLE",
        &format!("Revision {requested} is unavailable: only the latest revision ({current}) can be read or rendered; omit revision or pass {current}"),
    );
    error.0.current_revision = Some(current);
    error.0.field = Some("revision".into());
    error
}

fn request(name: &str, args: Value) -> Result<ReadRequest> {
    let mut req: ReadRequest = serde_json::from_value(args)?;
    if req.node_format.is_some() && name != "get_design_context" {
        return Err(crate::ApiError::invalid(
            "nodeFormat requires get_design_context",
        ));
    }
    req.compact = req.compact.filter(|on| *on);
    req.notes = req.notes.filter(|f| *f != notes::Filter::Open);
    req.instances = req.instances.filter(|mode| mode != "reference");
    req.states_mode = req.states_mode.filter(|mode| mode != "none");
    Ok(req)
}

fn list(
    state: &AppState,
    s: &fagent_storage::Snapshot,
    name: &str,
    req: ReadRequest,
) -> Result<Value> {
    let (scope, offset) = pagination::scope(s, name, &req)?;
    let items = match name {
        "list_fonts" => Value::Array(
            s.fonts
                .iter()
                .map(|f| crate::fonts::metadata(s, &f.id, &state.config.base_url))
                .collect::<Result<Vec<_>>>()?,
        ),
        "list_tokens" => serde_json::to_value(&s.tokens)?,
        "list_styles" => serde_json::to_value(&s.styles)?,
        "list_assets" => serde_json::to_value(&s.assets)?,
        _ => unreachable!("validated list tool"),
    };
    let entries: Vec<_> = items
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| {
            req.ids
                .as_ref()
                .is_none_or(|ids| ids.iter().any(|id| v["id"] == *id))
        })
        .cloned()
        .map(sanitize_asset)
        .map(|v| ("items".into(), v))
        .collect();
    let response =
        json!({"schemaVersion":1,"projectId":s.project_id,"revision":s.revision,"items":[]});
    pagination::page(
        response,
        entries,
        offset,
        req.limit.unwrap_or(100),
        65536,
        &scope,
    )
}
fn sanitize_asset(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.remove("relativeKey");
    }
    value
}
