//! Compares one batch applied in memory with what the database committed: revision,
//! changed ids, deleted rows and every touched row as canonical JSON. Rows are sets
//! keyed by `(kind, id)`: database order and `to_snapshot` order differ.
use super::rows::{before_rows, ram_row, snapshot_rows, Kind, Rows};
use super::Commit;
use fagent_node_memory::{Delta, ProjectState};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// How many differing ids a mismatch report names.
const REPORTED_IDS: usize = 5;

/// One disagreement between memory and the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Mismatch {
    pub reason: String,
    /// The first differing ids, sorted.
    pub ids: Vec<String>,
}

impl Mismatch {
    pub fn new<'a>(reason: impl Into<String>, ids: impl IntoIterator<Item = &'a String>) -> Self {
        Self {
            reason: reason.into(),
            ids: ids.into_iter().take(REPORTED_IDS).cloned().collect(),
        }
    }
}

fn lib_rows(delta: &Delta, kind: Kind) -> (BTreeSet<&String>, BTreeSet<&String>) {
    let (upserted, deleted): (&[String], &[String]) = match kind {
        Kind::Node => (&delta.nodes.upserted, &delta.nodes.deleted),
        Kind::Note => (&delta.notes.upserted, &delta.notes.deleted),
        Kind::Token => (&delta.tokens, &[]),
        Kind::Style => (&delta.styles, &[]),
        Kind::Asset | Kind::Font => (&[], &[]),
    };
    (upserted.iter().collect(), deleted.iter().collect())
}

/// Checks the staged batch against the committed one.
pub(crate) fn delta(state: &ProjectState, delta: &Delta, c: &Commit<'_>) -> Result<(), Mismatch> {
    if delta.revision != c.result.committed_revision {
        return Err(Mismatch::new("revision", []));
    }
    let lib: BTreeSet<String> = delta.changed_ids().into_iter().collect();
    let db: BTreeSet<String> = c.result.changed_ids.iter().cloned().collect();
    if lib != db {
        return Err(Mismatch::new("changed_ids", lib.symmetric_difference(&db)));
    }
    // Rows the lib touched that the database did not rewrite must equal the base row.
    let mut unchanged = BTreeSet::new();
    for kind in Kind::ALL {
        let (upserted, deleted) = lib_rows(delta, kind);
        let db_deleted: BTreeSet<&String> = c
            .rows
            .deletes
            .iter()
            .filter(|(k, _)| *k == kind)
            .map(|(_, id)| id)
            .collect();
        if deleted != db_deleted {
            let reason = format!("deleted.{}", kind.name());
            return Err(Mismatch::new(
                reason,
                deleted.symmetric_difference(&db_deleted).copied(),
            ));
        }
        let mut wrong = Vec::new();
        let written = c
            .rows
            .upserts
            .range((kind, String::new())..)
            .take_while(|((k, _), _)| *k == kind);
        for ((_, id), row) in written {
            if untracked(kind) || ram_row(state, kind, id).as_ref() != Some(row) {
                wrong.push(id);
            }
        }
        for id in upserted {
            if !c.rows.upserts.contains_key(&(kind, id.clone())) {
                unchanged.insert((kind, id.clone()));
            }
        }
        if !wrong.is_empty() {
            return Err(Mismatch::new(format!("row.{}", kind.name()), wrong));
        }
    }
    if unchanged.is_empty() {
        return Ok(());
    }
    let base = before_rows(c.before, &unchanged);
    let wrong = unchanged.iter().filter(|key| {
        let ram = ram_row(state, key.0, &key.1);
        ram.is_none() || base.get(*key) != ram.as_ref()
    });
    let ids: Vec<&String> = wrong.map(|(_, id)| id).collect();
    match ids.is_empty() {
        true => Ok(()),
        false => Err(Mismatch::new("row.unchanged", ids)),
    }
}

/// The lib does not expose asset or font rows and batches never write them.
fn untracked(kind: Kind) -> bool {
    matches!(kind, Kind::Asset | Kind::Font)
}

/// Strict check: the whole in-memory project equals the base rows with the
/// committed rows applied.
pub(crate) fn full(state: &ProjectState, c: &Commit<'_>) -> Result<(), Mismatch> {
    // Memory holds asset readiness as reads see it.
    let mut base = c.before.clone();
    if let Some(ready) = c.ready {
        ready(&mut base);
    }
    let mut expected = snapshot_rows(&base);
    apply(&mut expected, c.rows);
    let actual = snapshot_rows(&state.to_snapshot());
    if expected == actual {
        return Ok(());
    }
    let keys: BTreeSet<&(Kind, String)> = expected.keys().chain(actual.keys()).collect();
    let ids = keys
        .into_iter()
        .filter(|key| expected.get(*key) != actual.get(*key))
        .map(|(_, id)| id);
    Err(Mismatch::new("full", ids))
}

fn apply(rows: &mut BTreeMap<(Kind, String), Value>, commit: &Rows) {
    for key in &commit.deletes {
        rows.remove(key);
    }
    for (key, row) in &commit.upserts {
        rows.insert(key.clone(), row.clone());
    }
}
