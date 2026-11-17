//! `import_asset` takes the asset bytes from exactly one source: an inbox `path`, inline
//! `base64`, or `sourceProjectId` with `assetId` or `fontId`. As with `render_frame`,
//! clients see the flat union of these shapes while the server validates the strict form,
//! because MCP clients reject a top-level `oneOf`.
//!
//! `import_assets` is a separate tool rather than an `items` array on `import_asset`, so
//! neither tool's published shape or result is conditional: each of its `items` is the
//! same source union without the commit fields, published flat and checked per item by
//! `item_validation`, so an error can name the item.
use super::{id, object, text};
use fagent_storage::MAX_IMPORT_ITEMS;
use serde_json::{json, Map, Value};

pub const DESCRIPTION: &str = "Import one asset (max 10 MiB) from one source: path (a file in inboxDir; preferred, keeps bytes out of the conversation), base64, or sourceProjectId plus assetId or fontId to copy it (fontId keeps its registration; no mediaType). No URLs. Returns asset and commit fields like apply_batch.";
pub const MANY_DESCRIPTION: &str = "Import 1–50 assets (64 MiB total) as one atomic commit and revision. Each item is one import_asset source (inboxDir path preferred) with mediaType/font. A bad item fails all, naming items[i]. Returns items [{asset,font?}] and commit fields.";
const REQUIRED: [&str; 3] = ["projectId", "expectedRevision", "idempotencyKey"];

fn common() -> Map<String, Value> {
    let common = json!({"projectId":id(),"expectedRevision":{"type":"integer","minimum":0},"idempotencyKey":id()});
    common.as_object().unwrap().clone()
}
/// Fields of the two byte sources: the declared type and an optional font registration.
fn bytes() -> Value {
    let font = object(
        json!({"id":id(),"family":text(256),"weight":{"type":"integer","minimum":1,"maximum":1000},"style":{"enum":["normal","italic"]},"license":{"type":["string","null"],"maxLength":1024}}),
        &["id", "family", "weight", "style"],
    );
    json!({"mediaType":{"enum":["image/png","image/jpeg","image/webp","font/ttf","font/otf","font/woff2"]},"font":font})
}
fn base64() -> Value {
    json!({"type":"string","minLength":4,"maxLength":13981016})
}
/// One strict source shape over `base`: every field is required except `font`.
fn shape(base: &Map<String, Value>, extra: Value) -> Value {
    let mut properties = base.clone();
    properties.extend(extra.as_object().unwrap().clone());
    let required: Vec<&str> = properties
        .keys()
        .map(String::as_str)
        .filter(|k| *k != "font")
        .collect();
    object(Value::Object(properties.clone()), &required)
}
/// The four sources, each closed to the others.
fn sources(base: &Map<String, Value>) -> Value {
    let with_bytes = |key: &str, schema: Value| {
        let mut extra = bytes();
        extra[key] = schema;
        shape(base, extra)
    };
    let copy = |key: &str| shape(base, json!({"sourceProjectId":text(128),key:text(128)}));
    json!({"oneOf":[with_bytes("path", text(4096)), with_bytes("base64", base64()), copy("assetId"), copy("fontId")]})
}
/// Every source field, none required.
fn flat(base: Map<String, Value>) -> Map<String, Value> {
    let mut properties = base;
    properties.extend(bytes().as_object().unwrap().clone());
    properties.insert("path".into(), text(4096));
    properties.insert("base64".into(), base64());
    for key in ["sourceProjectId", "assetId", "fontId"] {
        properties.insert(key.into(), text(128));
    }
    properties
}
/// What `tools/list` shows: every source, none required.
pub fn published() -> Value {
    object(Value::Object(flat(common())), &REQUIRED)
}
/// What `validate` enforces: exactly one source, each closed to the others.
pub fn validation() -> Value {
    sources(&common())
}
/// `import_assets`: the commit fields and 1–50 flat items.
pub fn many() -> Value {
    let item = object(Value::Object(flat(Map::new())), &[]);
    let mut properties = common();
    properties.insert(
        "items".into(),
        json!({"type":"array","minItems":1,"maxItems":MAX_IMPORT_ITEMS,"items":item}),
    );
    object(
        Value::Object(properties),
        &[REQUIRED.as_slice(), &["items"]].concat(),
    )
}
/// What each `import_assets` item must be: exactly one source, as for `import_asset`.
pub fn item_validation() -> Value {
    sources(&Map::new())
}

#[cfg(test)]
mod tests;
