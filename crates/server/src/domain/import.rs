//! `import_asset` takes its bytes inline as base64, from a file in the project's inbox, or
//! copied from another project's asset or font. All reach storage as the same bytes, so
//! replay and the asset hash ignore the source. `import_assets` resolves each item the same
//! way and commits them together (`many`).
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use fagent_storage::{FontRegistration, ImportAssetRequest};
use serde::Deserialize;
mod copy;
pub(super) mod many;

/// The fields every import shares: which project, at which revision, under which key.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Commit {
    project_id: String,
    expected_revision: u64,
    idempotency_key: String,
}

/// One asset's source, as `import_asset` takes it and as each `import_assets` item.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Item {
    media_type: Option<String>,
    base64: Option<String>,
    path: Option<String>,
    font: Option<FontRegistration>,
    source_project_id: Option<String>,
    asset_id: Option<String>,
    font_id: Option<String>,
}

enum Source {
    Bytes(Vec<u8>),
    Inbox(String),
}

/// An item whose type and registration are known; inbox bytes are read in storage's task.
struct Resolved {
    media_type: String,
    font: Option<FontRegistration>,
    source: Source,
}

impl Source {
    fn read(self, project: &fagent_storage::ProjectHandle) -> fagent_storage::Result<Vec<u8>> {
        match self {
            Source::Bytes(bytes) => Ok(bytes),
            Source::Inbox(path) => project.read_inbox_asset(&path),
        }
    }
}

/// Splits the commit fields from the source fields.
fn split(mut args: Value) -> Result<(Commit, Value)> {
    let object = args
        .as_object_mut()
        .ok_or_else(|| ApiError::invalid("Arguments must be an object"))?;
    let mut commit = serde_json::Map::new();
    for key in ["projectId", "expectedRevision", "idempotencyKey"] {
        if let Some(value) = object.remove(key) {
            commit.insert(key.into(), value);
        }
    }
    Ok((serde_json::from_value(Value::Object(commit))?, args))
}

async fn resolve(state: &AppState, item: Item) -> Result<Resolved> {
    let picked = (item.base64, item.path, item.source_project_id);
    let copy_ids = item.asset_id.is_some() || item.font_id.is_some();
    Ok(match (item.media_type, picked) {
        (Some(media_type), (Some(base64), None, None)) if !copy_ids => Resolved {
            media_type,
            font: item.font,
            source: Source::Bytes(decode(&base64)?),
        },
        (Some(media_type), (None, Some(path), None)) if !copy_ids => Resolved {
            media_type,
            font: item.font,
            source: Source::Inbox(path),
        },
        (None, (None, None, Some(project))) if item.font.is_none() => {
            let copied = copy::read(state, project, item.asset_id, item.font_id).await?;
            Resolved {
                media_type: copied.media_type,
                font: copied.font,
                source: Source::Bytes(copied.bytes),
            }
        }
        _ => {
            return Err(ApiError::invalid(
                "Send exactly one of path, base64 or sourceProjectId",
            ))
        }
    })
}

pub(super) async fn run(state: &AppState, args: Value) -> Result<Value> {
    let (commit, item) = split(args)?;
    let item: Item = serde_json::from_value(item)?;
    let Resolved {
        media_type,
        font,
        source,
    } = resolve(state, item).await?;
    let mut import = ImportAssetRequest {
        project_id: commit.project_id,
        expected_revision: commit.expected_revision,
        idempotency_key: commit.idempotency_key,
        media_type,
        bytes: Vec::new(),
        font,
    };
    let (id, expected) = (import.project_id.clone(), import.expected_revision);
    let result = state
        .project(id.clone(), move |p| {
            import.bytes = source.read(p)?;
            p.import_asset(import)
        })
        .await;
    Ok(with_commit_fields(serde_json::to_value(
        conflict::checked(state, &id, expected, result).await?,
    )?))
}

fn decode(base64: &str) -> Result<Vec<u8>> {
    if base64.len() > 13_981_016 {
        return Err(ApiError::invalid("Asset exceeds 10 MiB"));
    }
    let bytes = STANDARD
        .decode(base64)
        .map_err(|_| ApiError::invalid("Invalid base64 asset"))?;
    if bytes.len() > fagent_storage::MAX_ASSET_BYTES {
        return Err(ApiError::invalid("Asset exceeds 10 MiB"));
    }
    Ok(bytes)
}

/// import_asset answers like apply_batch, with the commit fields at the top level; `result`
/// still carries them for clients written against the earlier nested shape.
fn with_commit_fields(mut value: Value) -> Value {
    let commit = value.get("result").and_then(Value::as_object).cloned();
    if let (Some(commit), Some(top)) = (commit, value.as_object_mut()) {
        for (key, field) in commit {
            top.entry(key).or_insert(field);
        }
    }
    value
}
