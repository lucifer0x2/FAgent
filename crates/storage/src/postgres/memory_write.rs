//! The entity rows of a batch staged in memory, as `transaction::persist` writes
//! them. `Rows` lists only rows that differ from the base revision.
use super::transaction::Written;
use crate::Result;
use fagent_node_memory::Rows;

pub(super) fn written(rows: &Rows) -> Result<Written> {
    let mut out = Written {
        kinds: vec![],
        ids: vec![],
        payloads: vec![],
        removed_kinds: vec![],
        removed_ids: vec![],
    };
    let mut push = |kind: &str, id: &str, payload: String| {
        out.kinds.push(kind.into());
        out.ids.push(id.into());
        out.payloads.push(payload);
    };
    for node in &rows.nodes {
        push("node", &node.id, serde_json::to_string(node)?);
    }
    for token in &rows.tokens {
        push("token", &token.id, serde_json::to_string(token)?);
    }
    for style in &rows.styles {
        push("style", &style.id, serde_json::to_string(style)?);
    }
    for note in &rows.notes {
        push("note", &note.id, serde_json::to_string(note)?);
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
