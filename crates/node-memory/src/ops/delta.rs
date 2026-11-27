//! Rows a successful batch changed, for a persistence layer and for callers that
//! report changed ids.
use crate::store::Store;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};

/// Rows of one table: written (exist after the batch) and deleted (existed before
/// it and no longer exist). Both sorted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RowChanges {
    pub upserted: Vec<String>,
    pub deleted: Vec<String>,
}

/// Changes of one batch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Delta {
    /// Revision after the batch.
    pub revision: u64,
    pub nodes: RowChanges,
    pub notes: RowChanges,
    /// Upserted token ids (tokens are never deleted by a batch), sorted.
    pub tokens: Vec<String>,
    /// Upserted style ids, sorted.
    pub styles: Vec<String>,
    /// Node and note ids created and deleted within the batch: no row changes, but
    /// they count as changed ids. Sorted.
    pub transient: Vec<String>,
    /// Notes removed with their anchor node by `delete_subtree` and absent after the
    /// batch (`BatchResult::deleted_note_ids`); also listed in `changed_ids`. Sorted.
    pub deleted_with_nodes: Vec<String>,
}

impl Delta {
    /// Every id the batch touched, as `Candidate::apply` + `prune_links` report them
    /// (`BatchResult::changed_ids`): nodes (including renumbered siblings), notes,
    /// tokens, styles and transient ids. Sorted, unique.
    pub fn changed_ids(&self) -> Vec<String> {
        let rows = [&self.nodes, &self.notes];
        let ids: BTreeSet<&String> = rows
            .into_iter()
            .flat_map(|r| r.upserted.iter().chain(&r.deleted))
            .chain(&self.tokens)
            .chain(&self.styles)
            .chain(&self.transient)
            .collect();
        ids.into_iter().cloned().collect()
    }
}

fn rows(
    touched: HashMap<String, bool>,
    exists: impl Fn(&str) -> bool,
    transient: &mut Vec<String>,
) -> RowChanges {
    let mut out = RowChanges::default();
    for (id, existed) in touched {
        match (exists(&id), existed) {
            (true, _) => out.upserted.push(id),
            (false, true) => out.deleted.push(id),
            (false, false) => transient.push(id),
        }
    }
    out.upserted.sort_unstable();
    out.deleted.sort_unstable();
    out
}

pub(super) fn build(
    store: &Store,
    nodes: HashMap<String, bool>,
    notes: HashMap<String, bool>,
    tokens: BTreeSet<String>,
    styles: BTreeSet<String>,
    cascaded: BTreeSet<String>,
) -> Delta {
    let deleted_with_nodes = cascaded
        .into_iter()
        .filter(|id| !store.res.notes.contains_key(id))
        .collect();
    let mut transient = Vec::new();
    let nodes = rows(nodes, |id| store.contains(id), &mut transient);
    let notes = rows(notes, |id| store.res.notes.contains_key(id), &mut transient);
    transient.sort_unstable();
    Delta {
        revision: 0,
        nodes,
        notes,
        tokens: tokens.into_iter().collect(),
        styles: styles.into_iter().collect(),
        transient,
        deleted_with_nodes,
    }
}
