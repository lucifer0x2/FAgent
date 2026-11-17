//! One or more assets committed as a single revision, shared by the SQLite and PostgreSQL
//! backends: item checks, the canonical request behind idempotency, the candidate change and
//! the history operations. A one-item import keeps the original single-asset request and
//! history shape, so `import_asset` and a one-item `import_assets` replay each other and
//! history written before multi-asset imports stays valid.
use crate::*;

/// How item errors name their item. `import_asset` keeps its original field names;
/// `import_assets` prefixes them with `items[<index>].`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fields {
    Single,
    Indexed,
}

impl Fields {
    pub(crate) fn at(self, index: usize, mut error: StorageError) -> StorageError {
        if self == Self::Indexed {
            let field = error.field.take();
            error.field = Some(match field {
                Some(field) => format!("items[{index}].{field}"),
                None => format!("items[{index}]"),
            });
            error.message = format!("items[{index}]: {}", error.message);
        }
        error
    }
}

/// The checked items: each one's asset manifest, in request order, plus the canonical
/// request and its hash.
pub(crate) struct Prepared {
    pub assets: Vec<Asset>,
    pub canonical: Value,
    pub request_hash: String,
}

pub(crate) fn prepare(request: &ImportAssetsRequest, fields: Fields) -> Result<Prepared> {
    let items = &request.items;
    if items.is_empty() || items.len() > MAX_IMPORT_ITEMS {
        return Err(StorageError::validation(
            "items",
            &format!("Import 1–{MAX_IMPORT_ITEMS} assets per call"),
        ));
    }
    let total: usize = items.iter().map(|item| item.bytes.len()).sum();
    if total > MAX_IMPORT_BYTES {
        return Err(StorageError::validation(
            "items",
            "Assets exceed 64 MiB in total; split them into several calls",
        ));
    }
    let mut assets = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let asset = manifest(item).map_err(|e| fields.at(index, e))?;
        let earlier = assets.iter().position(|a: &Asset| a.id == asset.id);
        if earlier.is_some_and(|i| assets[i].media_type != asset.media_type) {
            let error = StorageError::validation("mediaType", "Same bytes as an earlier item");
            return Err(fields.at(index, error));
        }
        assets.push(asset);
    }
    let entries: Vec<_> = items
        .iter()
        .zip(&assets)
        .map(|(item, asset)| {
            (
                asset.media_type.as_str(),
                asset.content_hash.as_str(),
                &item.font,
            )
        })
        .collect();
    let canonical = canonical(&request.project_id, request.expected_revision, &entries);
    let request_hash = hash(&serde_json::to_vec(&canonical)?);
    Ok(Prepared {
        assets,
        canonical,
        request_hash,
    })
}

fn manifest(item: &ImportAssetItem) -> Result<Asset> {
    if item.bytes.is_empty() || item.bytes.len() > MAX_ASSET_BYTES {
        return Err(StorageError::validation(
            "bytes",
            "Asset must contain 1 byte to 10 MiB",
        ));
    }
    let (width, height) = crate::assets::asset_dimensions(&item.media_type, &item.bytes)?;
    let content_hash = hash(&item.bytes);
    Ok(Asset {
        id: format!("asset_{}", &content_hash[..32]),
        content_hash: content_hash.clone(),
        media_type: item.media_type.clone(),
        byte_size: item.bytes.len() as u64,
        relative_key: content_hash,
        width,
        height,
        status: "ready".into(),
    })
}

/// The request whose hash guards the idempotency key: content hashes, never bytes or the
/// source an item came from. One item keeps the original single-asset shape.
pub(crate) fn canonical(
    project: &str,
    expected: u64,
    items: &[(&str, &str, &Option<FontRegistration>)],
) -> Value {
    let entry = |(media, hash, font): &(&str, &str, &Option<FontRegistration>)| json!({"mediaType":media,"hash":hash,"font":font});
    let mut request = match items {
        [one] => entry(one),
        _ => json!({"items":items.iter().map(entry).collect::<Vec<_>>()}),
    };
    request["projectId"] = json!(project);
    request["expectedRevision"] = json!(expected);
    canonical_json(&request)
}

/// Adds every asset and font registration to the candidate and returns the changed IDs,
/// each once, in item order. A font ID may be registered by one item only.
pub(crate) fn apply(
    candidate: &mut Candidate,
    request: &ImportAssetsRequest,
    assets: &[Asset],
    fields: Fields,
) -> Result<Vec<String>> {
    let mut changed: Vec<String> = Vec::new();
    let mut fonts: Vec<&str> = Vec::new();
    for (index, (item, asset)) in request.items.iter().zip(assets).enumerate() {
        candidate.assets.insert(asset.id.clone(), asset.clone());
        if !changed.contains(&asset.id) {
            changed.push(asset.id.clone());
        }
        if let Some(font) = &item.font {
            if let Some(earlier) = fonts.iter().position(|id| *id == font.id) {
                let error = StorageError::validation(
                    "font",
                    &format!(
                        "Font ID {} is already registered by items[{earlier}]",
                        font.id
                    ),
                );
                return Err(fields.at(index, error));
            }
            fonts.push(&font.id);
        }
        crate::assets::register_font(
            candidate,
            &item.media_type,
            item.font.as_ref(),
            asset,
            &mut changed,
        )
        .map_err(|e| fields.at(index, e))?;
    }
    Ok(changed)
}

/// History operations: one internal `import_asset` entry per item, in order.
pub(crate) fn history(request: &ImportAssetsRequest, assets: &[Asset]) -> Value {
    let entries: Vec<Value> = request
        .items
        .iter()
        .zip(assets)
        .map(|(item, asset)| json!({"op":"import_asset","asset":asset,"font":item.font}))
        .collect();
    Value::Array(entries)
}

/// Whether stored history operations are a resource import: 1–`MAX_IMPORT_ITEMS` entries,
/// all `import_asset`.
pub(crate) fn is_import(operations: &Value) -> bool {
    operations.as_array().is_some_and(|ops| {
        !ops.is_empty()
            && ops.len() <= MAX_IMPORT_ITEMS
            && ops.iter().all(|op| op["op"] == "import_asset")
    })
}

/// The canonical request that stored history of an import must hash to, rebuilt from its
/// `import_asset` entries at `base`.
pub(crate) fn replay_request(project: &str, base: u64, operations: &Value) -> Result<Value> {
    if !is_import(operations) {
        return Err(bundle::invalid("Invalid resource commit"));
    }
    let mut entries = Vec::new();
    for op in operations.as_array().into_iter().flatten() {
        let asset: Asset = serde_json::from_value(op["asset"].clone())?;
        let font: Option<FontRegistration> = serde_json::from_value(op["font"].clone())?;
        entries.push((asset.media_type, asset.content_hash, font));
    }
    let entries: Vec<_> = entries
        .iter()
        .map(|(media, hash, font)| (media.as_str(), hash.as_str(), font))
        .collect();
    Ok(canonical(project, base, &entries))
}

/// Each content hash once, in item order: the binaries an import writes.
pub(crate) fn unique_binaries<'a>(
    request: &'a ImportAssetsRequest,
    assets: &'a [Asset],
) -> Vec<(&'a Asset, &'a [u8])> {
    let mut seen = BTreeSet::new();
    request
        .items
        .iter()
        .zip(assets)
        .filter(|(_, asset)| seen.insert(asset.content_hash.as_str()))
        .map(|(item, asset)| (asset, item.bytes.as_slice()))
        .collect()
}
