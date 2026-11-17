//! `import_assets`: each item is resolved like an `import_asset` source, then storage commits
//! every asset (and font registration) atomically as one revision. Any invalid item fails the
//! whole call; errors name it in `field` (`items[<index>]…`) and the message.
use super::*;
use fagent_storage::{ImportAssetItem, ImportAssetsRequest, StorageError, MAX_IMPORT_BYTES};

/// Names item `index` in an error raised while resolving or reading it. Storage errors from
/// the commit itself already carry the index.
fn at(index: usize, mut error: ApiError, field: &str) -> ApiError {
    let inner = &mut error.0;
    inner.message = format!("items[{index}]: {}", inner.message);
    let field = inner.field.take().unwrap_or_else(|| field.into());
    inner.field = Some(format!("items[{index}].{field}"));
    error
}

fn too_large() -> StorageError {
    StorageError::validation(
        "items",
        "Assets exceed 64 MiB in total; split them into several calls",
    )
}

pub(crate) async fn run(state: &AppState, args: Value) -> Result<Value> {
    let (commit, mut rest) = split(args)?;
    let items = match rest["items"].take() {
        Value::Array(items) => items,
        _ => return Err(ApiError::invalid("items must be an array of 1–50 assets")),
    };
    let mut resolved = Vec::with_capacity(items.len());
    let mut held = 0usize;
    for (index, item) in items.into_iter().enumerate() {
        crate::schemas::validate_import_item(&item).map_err(|e| {
            let e = ApiError::invalid(&format!(
                "{} (send exactly one of path, base64 or sourceProjectId with assetId or fontId; mediaType and font only with path or base64)",
                e.0.message
            ));
            at(index, e, "source")
        })?;
        let item: Item = serde_json::from_value(item).map_err(|e| at(index, e.into(), "source"))?;
        let item = resolve(state, item)
            .await
            .map_err(|e| at(index, e, "source"))?;
        if let Source::Bytes(bytes) = &item.source {
            held += bytes.len();
            if held > MAX_IMPORT_BYTES {
                return Err(too_large().into());
            }
        }
        resolved.push(item);
    }
    let fonts: Vec<Option<FontRegistration>> = resolved.iter().map(|r| r.font.clone()).collect();
    let (id, expected) = (commit.project_id.clone(), commit.expected_revision);
    let result = state
        .project(id.clone(), move |p| {
            let mut items = Vec::with_capacity(resolved.len());
            let mut total = 0usize;
            for (index, item) in resolved.into_iter().enumerate() {
                let bytes = item.source.read(p).map_err(|e| {
                    let e = at(index, e.into(), "path");
                    *e.0
                })?;
                total += bytes.len();
                if total > MAX_IMPORT_BYTES {
                    return Err(too_large());
                }
                items.push(ImportAssetItem {
                    media_type: item.media_type,
                    bytes,
                    font: item.font,
                });
            }
            p.import_assets(ImportAssetsRequest {
                project_id: commit.project_id,
                expected_revision: commit.expected_revision,
                idempotency_key: commit.idempotency_key,
                items,
            })
        })
        .await;
    let imported = conflict::checked(state, &id, expected, result).await?;
    let items: Vec<Value> = imported
        .assets
        .iter()
        .zip(fonts)
        .map(|(asset, font)| match font {
            Some(font) => json!({"asset":asset,"font":font}),
            None => json!({"asset":asset}),
        })
        .collect();
    let mut response = serde_json::to_value(imported.result)?;
    response["items"] = Value::Array(items);
    Ok(response)
}
