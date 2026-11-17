//! The `(id, key)` pairs an operation array writes, read from its JSON so stored
//! history of any age and a new request are interpreted by the same code.
use serde_json::Value;
use std::collections::BTreeSet;

/// `(entity id, key)` pairs: a top-level props key, `placement` (explicit
/// reparent/reorder/indexed create), `created`, `deleted`, or `token`/`style`/
/// `note` for resources. Renumbered siblings are a side effect and never listed.
pub(crate) type Writes = BTreeSet<(String, String)>;

fn pair(id: &Value, key: &str) -> Option<(String, String)> {
    Some((id.as_str()?.to_owned(), key.to_owned()))
}

/// Pairs one stored or requested operation array writes; `None` for unknown or
/// malformed history. A typed resource import (one or several assets) writes no node keys.
pub(crate) fn operation_writes(operations: &Value) -> Option<Vec<(String, String)>> {
    if crate::asset_import::is_import(operations) {
        // Both backends and bundle history store {op,asset,font} per asset, with font
        // null for image imports. This opcode is internal history, not a batch API op.
        for op in operations.as_array()? {
            let op = op.as_object()?;
            if op.len() != 3 {
                return None;
            }
            let _: crate::Asset = serde_json::from_value(op.get("asset")?.clone()).ok()?;
            let _: Option<crate::FontRegistration> =
                serde_json::from_value(op.get("font")?.clone()).ok()?;
        }
        return Some(Vec::new());
    }
    let operations = operations.as_array()?;
    let mut out = Vec::new();
    for op in operations {
        let id = &op["id"];
        match op["op"].as_str()? {
            "create_node" => {
                out.extend(pair(id, "created"));
                if !op["index"].is_null() {
                    out.extend(pair(id, "placement"));
                }
            }
            "update_node" => {
                for key in op["props"].as_object()?.keys() {
                    out.extend(pair(id, key));
                }
            }
            "reparent_node" | "reorder_node" => out.extend(pair(id, "placement")),
            "delete_subtree" => out.extend(pair(id, "deleted")),
            "upsert_token" => out.extend(pair(&op["token"]["id"], "token")),
            "upsert_style" => out.extend(pair(&op["style"]["id"], "style")),
            "create_note" | "reply_note" | "update_note" => out.extend(pair(id, "note")),
            "delete_note" => out.extend(pair(id, "deleted")),
            _ => return None,
        }
    }
    Some(out)
}

/// What this batch writes: its operations plus every node or note it deleted,
/// including descendants and notes removed with a subtree. `changed` is the
/// batch's changed ids; `exists_after` tells whether an id still exists.
pub(crate) fn batch_writes(
    operations: &Value,
    changed: &[String],
    exists_after: impl Fn(&str) -> bool,
) -> Writes {
    let mut out: Writes = operation_writes(operations)
        .unwrap_or_default()
        .into_iter()
        .collect();
    // Ids this batch created are not at the base: created then deleted is no write.
    let created: BTreeSet<&str> = operations
        .as_array()
        .into_iter()
        .flatten()
        .filter(|op| {
            matches!(
                op["op"].as_str(),
                Some("create_node" | "create_note" | "reply_note")
            )
        })
        .filter_map(|op| op["id"].as_str())
        .collect();
    let deleted: Vec<String> = changed
        .iter()
        .filter(|id| !created.contains(id.as_str()) && !exists_after(id))
        .cloned()
        .collect();
    out.extend(deleted.into_iter().map(|id| (id, "deleted".to_owned())));
    out
}
