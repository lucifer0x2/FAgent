//! Task28 I3 on PostgreSQL: where a commit's base comes from and the rows it
//! persists when it was staged in memory.
//!
//! The whole commit runs in one pool job on one worker thread: the project row lock
//! (`FOR UPDATE`) orders writers of a project across processes, and the staged
//! batch's `std::sync` write guard is created and released on that thread, with no
//! `.await` anywhere in storage. The staged batch is committed after the database
//! commit, so the next writer (waiting on the row lock, then on the project's read
//! lock) sees memory at the revision the database reports.
use super::snapshot::{self, Binaries, Stored};
use super::transaction::Written;
use super::*;
use crate::shadow::{MemoryBase, MemoryRows, Shadow};
use crate::{Snapshot, SnapshotCacheIdentity, StorageError};
use std::collections::{HashMap, HashSet};

/// Internal: the lib rejected a batch the validator accepted. The adapter evicts
/// the project and runs the batch again on today's path; never returned to callers.
pub(super) const FALLBACK: &str = "NODE_MEMORY_FALLBACK";

pub(super) fn fallback() -> StorageError {
    StorageError::new(FALLBACK, "Retry on the database path")
}

/// Where a commit's base snapshot came from.
pub(super) enum Source {
    /// Today's path: stored rows (with their text) and binary hashes.
    Database {
        binaries: Binaries,
        stored: Stored,
    },
    Memory(MemoryBase),
}

/// The base snapshot for a commit at `identity`: memory when writes are on and it
/// holds this incarnation and revision, else today's full load.
pub(super) fn load<C: ::postgres::GenericClient>(
    db: &mut Db<'_, C>,
    project: projects::ProjectRow,
    shadow: Option<&Shadow>,
    identity: &SnapshotCacheIdentity,
) -> Result<(Snapshot, Source)> {
    if let Some(found) = shadow.and_then(|s| s.memory_base(identity)) {
        let (before, memory) = found;
        return Ok((before, Source::Memory(memory)));
    }
    let (before, binaries, stored) =
        crate::bench_phase::time("load", || snapshot::entities_for_write(db, project))?;
    Ok((before, Source::Database { binaries, stored }))
}

/// The base row text of `ids` in one table, by one pass over it.
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
pub(super) fn written(before: &Snapshot, rows: &MemoryRows) -> Result<Written> {
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
