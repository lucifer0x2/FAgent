//! Write operations and the atomic batch that applies them. Input is the stored
//! `Operation`; every primitive change is logged, and a failing (or panicking) batch
//! unwinds the log, so state and indexes return exactly to their previous values
//! without cloning the project.
mod create;
mod delete;
mod delta;
mod finish;
mod notes;
mod place;
mod resources;
mod tree;
mod update;

use crate::store::{Key, Log, Store};
pub use delta::{Delta, RowChanges};
use fagent_scene_input::{Operation, Result};
use std::collections::{BTreeSet, HashMap};

/// A batch in progress. Dropping it before [`Txn::into_parts`] rolls everything back.
pub(crate) struct Txn<'a> {
    store: &'a mut Store,
    log: Log,
    /// Node id -> whether it existed before the batch (first touch wins).
    nodes: HashMap<String, bool>,
    /// Note id -> whether it existed before the batch (first touch wins).
    notes: HashMap<String, bool>,
    tokens: BTreeSet<String>,
    styles: BTreeSet<String>,
    /// Some `update_node` removed `component` from a master.
    unmarked: bool,
    /// Siblings renumbered by placements; touched (as existing) when the batch ends.
    renumbered: Vec<Key>,
    max_nodes: usize,
    /// Stamp of the pending revision (offset from the load revision).
    rev: u32,
    armed: bool,
}

impl<'a> Txn<'a> {
    pub(crate) fn new(store: &'a mut Store, max_nodes: usize, rev: u32) -> Self {
        Self {
            store,
            log: Vec::new(),
            nodes: HashMap::new(),
            notes: HashMap::new(),
            tokens: BTreeSet::new(),
            styles: BTreeSet::new(),
            renumbered: Vec::new(),
            unmarked: false,
            max_nodes,
            rev,
            armed: true,
        }
    }

    pub(crate) fn apply(&mut self, op: &Operation) -> Result<()> {
        match op {
            Operation::CreateNode {
                id,
                kind,
                parent_id,
                props,
                index,
            } => self.create(id, kind, parent_id.as_deref(), props, *index),
            Operation::UpdateNode { id, props, merge } => self.update(id, props, *merge),
            Operation::ReparentNode {
                id,
                parent_id,
                index,
            } => self.reparent(id, parent_id, *index),
            Operation::ReorderNode { id, index } => self.reorder(id, *index),
            Operation::DeleteSubtree { id } => self.delete(id),
            Operation::UpsertToken { token } => self.upsert_token(token),
            Operation::UpsertStyle { style } => self.upsert_style(style),
            Operation::CreateNote { .. }
            | Operation::ReplyNote { .. }
            | Operation::UpdateNote { .. }
            | Operation::DeleteNote { .. } => self.note(op),
        }
    }

    fn touch(&mut self, id: &str, existed: bool) {
        if !self.nodes.contains_key(id) {
            self.nodes.insert(id.to_owned(), existed);
        }
    }

    fn touch_owned(&mut self, id: String, existed: bool) {
        self.nodes.entry(id).or_insert(existed);
    }

    /// Marks a node that existed when it was first touched.
    fn touch_key(&mut self, key: Key) {
        let id = &self.store.node(key).id;
        if !self.nodes.contains_key(id) {
            self.nodes.insert(id.clone(), true);
        }
    }

    fn touch_note(&mut self, id: &str, existed: bool) {
        if !self.notes.contains_key(id) {
            self.notes.insert(id.to_owned(), existed);
        }
    }

    /// Keeps the changes: returns the undo log (to roll back later) and the changed
    /// rows. The revision of the delta is set by the caller.
    pub(crate) fn into_parts(mut self) -> (Log, Delta) {
        self.armed = false;
        // A renumbered sibling deleted later was touched by the delete; a slot reused
        // since then holds a node created (and touched) in this batch.
        for key in std::mem::take(&mut self.renumbered) {
            if let Some(slot) = self.store.live(key) {
                if !self.nodes.contains_key(&slot.node.id) {
                    self.nodes.insert(slot.node.id.clone(), true);
                }
            }
        }
        let delta = delta::build(
            self.store,
            std::mem::take(&mut self.nodes),
            std::mem::take(&mut self.notes),
            std::mem::take(&mut self.tokens),
            std::mem::take(&mut self.styles),
        );
        (std::mem::take(&mut self.log), delta)
    }
}

impl Drop for Txn<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.store.rollback(std::mem::take(&mut self.log));
        }
    }
}
