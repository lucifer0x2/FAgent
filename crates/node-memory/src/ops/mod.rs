//! Write operations and the atomic batch that applies them. Input is the stored
//! `Operation`. A batch runs on an O(1) clone of the project state: a failing (or
//! panicking) batch drops the clone, so the previous version never changes.
mod create;
mod delete;
mod delta;
mod finish;
mod notes;
mod place;
mod rejection;
mod resources;
mod tree;
mod update;

use crate::store::{Key, Store};
pub use delta::{Delta, RowChanges};
use fagent_scene_input::Operation;
pub(crate) use rejection::Rejection;
use std::collections::{BTreeSet, HashMap};

/// A batch in progress on a cloned store; dropping the clone discards it.
pub(crate) struct Txn<'a> {
    store: &'a mut Store,
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
}

impl<'a> Txn<'a> {
    pub(crate) fn new(store: &'a mut Store, max_nodes: usize, rev: u32) -> Self {
        Self {
            store,
            nodes: HashMap::new(),
            notes: HashMap::new(),
            tokens: BTreeSet::new(),
            styles: BTreeSet::new(),
            renumbered: Vec::new(),
            unmarked: false,
            max_nodes,
            rev,
        }
    }

    /// Applies one operation with `Candidate::apply`'s checks, in its order, on the
    /// working state (see [`Rejection`] for the few that differ). After a token upsert
    /// every error is inexact (see [`Self::deferred`]).
    pub(crate) fn apply(&mut self, op: &Operation) -> std::result::Result<(), Rejection> {
        let deferred = self.deferred();
        self.run(op).map_err(|rejection| match deferred {
            true => rejection.routed(),
            false => rejection,
        })
    }

    /// Whether the batch upserted a token. Token validation then waits for `finish`:
    /// `Candidate::apply` validates every token after each upsert, which costs
    /// O(operations x tokens) per batch. Until `finish` the token set may be one
    /// `Candidate` already rejected, so a later error is not necessarily its error,
    /// and a batch whose token set is repaired by a later upsert is accepted here.
    /// The engine runs the reference on every token batch (the delta classifier does
    /// not support them), which reports `Candidate`'s exact error in both cases.
    pub(crate) fn deferred(&self) -> bool {
        !self.tokens.is_empty()
    }

    fn run(&mut self, op: &Operation) -> std::result::Result<(), Rejection> {
        match op {
            Operation::CreateNode {
                id,
                kind,
                parent_id,
                props,
                index,
            } => self.create(id, kind, parent_id.as_deref(), props, *index)?,
            Operation::UpdateNode { id, props, merge } => self.update(id, props, *merge)?,
            Operation::ReparentNode {
                id,
                parent_id,
                index,
            } => self.reparent(id, parent_id, *index)?,
            Operation::ReorderNode { id, index } => self.reorder(id, *index)?,
            Operation::DeleteSubtree { id } => self.delete(id)?,
            Operation::UpsertToken { token } => self.upsert_token(token)?,
            Operation::UpsertStyle { style } => self.upsert_style(style)?,
            Operation::CreateNote { .. }
            | Operation::ReplyNote { .. }
            | Operation::UpdateNote { .. }
            | Operation::DeleteNote { .. } => self.note(op)?,
        }
        Ok(())
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

    /// The changed rows. The revision of the delta is set by the caller.
    pub(crate) fn into_delta(mut self) -> Delta {
        // A renumbered sibling deleted later was touched by the delete; one recreated
        // under the same id since then was touched by its create.
        for key in std::mem::take(&mut self.renumbered) {
            if let Some(slot) = self.store.live(key) {
                if !self.nodes.contains_key(&slot.node.id) {
                    self.nodes.insert(slot.node.id.clone(), true);
                }
            }
        }
        delta::build(
            self.store,
            std::mem::take(&mut self.nodes),
            std::mem::take(&mut self.notes),
            std::mem::take(&mut self.tokens),
            std::mem::take(&mut self.styles),
        )
    }
}
