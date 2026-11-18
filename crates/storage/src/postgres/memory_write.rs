//! The rows a batch staged in memory persists on PostgreSQL. Unused until
//! PostgreSQL commits stage in `NodeMemory`.
use super::transaction::Written;
use crate::{Result, Snapshot};
use fagent_node_memory::Rows;
use std::collections::{HashMap, HashSet};

/// The base row text of `ids` in one table, by one pass over it.
#[allow(dead_code)]
fn base_text<'a, T: serde::Serialize>(
    rows: &'a [T],
    ids: impl Iterator<Item = &'a str>,
    id: impl Fn(&T) -> &str,
) -> Result<HashMap<&'a str, String>> {
    let wanted: HashSet<&str> = ids.collect();
    let mut out = HashMap::new();
    for row in rows.iter().filter(|row| wanted.contains(id(row))) {
        let text = serde_json::to_string(row)?;
        out.insert(wanted.get(id(row)).copied().unwrap_or_default(), text);
    }
    Ok(out)
}

/// The entity rows of a batch staged in memory, as `persist` writes them. Like
/// `Stored::delta`, a row whose text equals the base row's is not rewritten.
#[allow(dead_code)]
pub(super) fn written(before: &Snapshot, rows: &Rows) -> Result<Written> {
    let nodes = base_text(
        &before.nodes,
        rows.nodes.iter().map(|n| n.id.as_str()),
        |n| &n.id,
    )?;
    let tokens = base_text(
        &before.tokens,
        rows.tokens.iter().map(|t| t.id.as_str()),
        |t| &t.id,
    )?;
    let styles = base_text(
        &before.styles,
        rows.styles.iter().map(|s| s.id.as_str()),
        |s| &s.id,
    )?;
    let notes = base_text(
        &before.notes,
        rows.notes.iter().map(|n| n.id.as_str()),
        |n| &n.id,
    )?;
    let mut out = Written {
        kinds: vec![],
        ids: vec![],
        payloads: vec![],
        removed_kinds: vec![],
        removed_ids: vec![],
    };
    let mut push = |kind: &str, id: &str, payload: String, base: &HashMap<&str, String>| {
        if base.get(id) == Some(&payload) {
            return;
        }
        out.kinds.push(kind.into());
        out.ids.push(id.into());
        out.payloads.push(payload);
    };
    for node in &rows.nodes {
        push("node", &node.id, serde_json::to_string(node)?, &nodes);
    }
    for token in &rows.tokens {
        push("token", &token.id, serde_json::to_string(token)?, &tokens);
    }
    for style in &rows.styles {
        push("style", &style.id, serde_json::to_string(style)?, &styles);
    }
    for note in &rows.notes {
        push("note", &note.id, serde_json::to_string(note)?, &notes);
    }
    let removed = [("node", &rows.deleted_nodes), ("note", &rows.deleted_notes)];
    for (kind, ids) in removed {
        for id in ids {
            out.removed_kinds.push(kind.into());
            out.removed_ids.push(id.clone());
        }
    }
    Ok(out)
}
