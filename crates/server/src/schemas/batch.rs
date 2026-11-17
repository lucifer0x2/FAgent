//! `apply_batch` takes `operations` (JSON), `source` (OCD, compiled to the same operations), or
//! both (source first, one batch). Like `render_frame`, clients see a flat object because MCP
//! and the Anthropic API reject top-level combinators; the server validates the strict form.
use super::*;

pub(super) const INLINE_DESCRIPTION: &str = "Atomically apply 1–500 ordered JSON operations (2 MiB). update_node merge:true: props are a JSON merge patch (null deletes). Or send source: OCD line DSL (skill://fagent/ocd.md) compiled to the same operations, ahead of any operations; parse errors return SOURCE_PARSE_FAILED with line/column. Last writer wins per top-level props key; parent+position is one property. Pass expectedRevision from your read: results list concurrent (≤20 {nodeId,key,revision} written since and overwritten here; concurrentTotal, or concurrentUnknown). Only invalid batches fail, naming node, field and current value. strict:true rejects a stale expectedRevision (REVISION_CONFLICT; with pageIds, only for those pages). Replay identical key/body after lost responses; changed bodies need new keys. dryRun:true validates without writes.";
pub(super) const FILE_DESCRIPTION: &str = "Apply one batch file from the project inbox (inboxDir); preferred for large or iterative batches (dryRun, fix, apply). A .ocd file is OCD source compiled like apply_batch source (line/column are the file's); other names hold a pure JSON operation array. Same validation, revision, idempotency, last-writer-wins, concurrent and strict rules as apply_batch. path is inbox-relative or absolute inside that inbox; traversal and symlinks forbidden. Max 2 MiB, 1–500 operations; no code execution or automatic chunking. Hash uses canonical content, not filename; changed content needs a new key.";
const REQUIRED: [&str; 2] = ["projectId", "idempotencyKey"];
/// OCD source shares the 2 MiB cap of an inline batch. JSON Schema counts characters; the
/// compiler also enforces it in UTF-8 bytes (`ocd::apply`).
const SOURCE_MAX: u64 = 2 * 1024 * 1024;

fn common() -> Value {
    json!({"projectId":id(),"expectedRevision":{"type":"integer","minimum":0},
        "pageIds":{"type":"array","minItems":1,"maxItems":100,"uniqueItems":true,"items":id()},
        "idempotencyKey":id(),"dryRun":{"type":"boolean"},"strict":{"type":"boolean"}})
}
fn operations() -> Value {
    json!({"type":"array","minItems":1,"maxItems":500,"items":operations::schema()})
}
/// Only called after a failed `apply_batch` validation, to point at the failing operation.
pub(super) fn invalid_operation(items: &[Value]) -> Option<usize> {
    let schema = operations::schema();
    items
        .iter()
        .position(|op| validation::validate(&schema, op).is_err())
}
/// What `tools/list` shows: both inputs, neither required.
pub(super) fn inline() -> Value {
    let mut properties = common();
    properties["operations"] = operations();
    properties["source"] = text(SOURCE_MAX);
    object(properties, &REQUIRED)
}
/// What `validate` enforces: `operations`, `source` or both (the combined count is checked
/// after compiling).
pub(super) fn inline_validation() -> Value {
    let mut with_operations = common();
    with_operations["operations"] = operations();
    let mut with_source = common();
    with_source["source"] = text(SOURCE_MAX);
    let mut with_both = with_operations.clone();
    with_both["source"] = text(SOURCE_MAX);
    let required = |inputs: &[&'static str]| [REQUIRED.as_slice(), inputs].concat();
    json!({"oneOf":[
        object(with_operations, &required(&["operations"])),
        object(with_source, &required(&["source"])),
        object(with_both, &required(&["source", "operations"])),
    ]})
}
pub(super) fn file() -> Value {
    let mut properties = common();
    properties["path"] = text(4096);
    object(properties, &["projectId", "idempotencyKey", "path"])
}
