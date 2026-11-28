//! Note operations on a candidate, the `delete_subtree` cascade and whole-document note checks.
use super::{values::id as check_id, Candidate, Lookup};
use crate::{error::*, model::*};
use std::collections::BTreeSet;

impl Candidate {
    pub(super) fn apply_note(
        &mut self,
        op: &Operation,
        changed: &mut BTreeSet<String>,
    ) -> Result<()> {
        let key = match op {
            Operation::CreateNote {
                id,
                node_id,
                text,
                author,
            } => {
                check_id(id)?;
                if self.notes.contains_key(id) {
                    return Err(invalid("id", "Note ID already exists"));
                }
                if self.nodes.contains_key(id) {
                    return Err(invalid("id", "ID already belongs to a node"));
                }
                self.anchor(node_id)?;
                let note = Note {
                    id: id.clone(),
                    node_id: node_id.clone(),
                    text: content(text)?,
                    author: author_of(author)?,
                    resolved: false,
                    replies: Vec::new(),
                };
                self.notes.insert(id.clone(), note);
                id
            }
            Operation::ReplyNote { id, text, author } => {
                let reply = NoteReply {
                    text: content(text)?,
                    author: author_of(author)?,
                };
                let note = self.note_mut(id)?;
                let limits = note_limits();
                if note.replies.len() >= limits.replies {
                    return Err(limits.replies_full());
                }
                note.replies.push(reply);
                id
            }
            Operation::UpdateNote {
                id,
                text,
                resolved,
                node_id,
            } => {
                self.note_mut(id)?;
                if text.is_none() && resolved.is_none() && node_id.is_none() {
                    return Err(invalid("update_note", "Set text, resolved or nodeId"));
                }
                if let Some(node) = node_id {
                    self.anchor(node)?;
                }
                let text = text.as_deref().map(content).transpose()?;
                let note = self.note_mut(id)?;
                if let Some(text) = text {
                    note.text = text;
                }
                if let Some(resolved) = resolved {
                    note.resolved = *resolved;
                }
                if let Some(node) = node_id {
                    note.node_id = node.clone();
                }
                id
            }
            Operation::DeleteNote { id } => {
                self.note_mut(id)?;
                self.notes.remove(id);
                id
            }
            _ => return Err(invalid("op", "Not a note operation")),
        };
        changed.insert(key.clone());
        Ok(())
    }

    /// Removes the notes anchored to `removed` nodes; their IDs become changed (now removed).
    pub(super) fn cascade_notes(
        &mut self,
        removed: &BTreeSet<String>,
        changed: &mut BTreeSet<String>,
    ) {
        let cascaded = &mut self.cascaded_notes;
        self.notes.retain(|id, note| {
            let keep = !removed.contains(&note.node_id);
            if !keep {
                changed.insert(id.clone());
                cascaded.insert(id.clone());
            }
            keep
        });
    }

    /// Whole-document checks, shared by batches and bundle imports.
    pub(super) fn validate_notes(&self) -> Result<()> {
        let limits = note_limits();
        if self.notes.len() > limits.notes {
            return Err(limits.too_many_notes());
        }
        for (key, note) in &self.notes {
            check_id(&note.id)?;
            if key != &note.id {
                return Err(not_unique());
            }
            check_note(self, note)?;
        }
        Ok(())
    }

    fn anchor(&self, node: &str) -> Result<()> {
        if self.nodes.contains_key(node) {
            Ok(())
        } else {
            Err(invalid(
                "nodeId",
                "Note must be anchored to an existing node",
            ))
        }
    }

    fn note_mut(&mut self, id: &str) -> Result<&mut Note> {
        self.notes
            .get_mut(id)
            .ok_or_else(|| invalid("id", "Note does not exist"))
    }
}

fn not_unique() -> StorageError {
    invalid("id", "Note ID must be unique and not a node ID")
}

/// The whole-document checks of one stored note, in `validate_notes`' order.
pub(super) fn check_note(lookup: &dyn Lookup, note: &Note) -> Result<()> {
    check_id(&note.id)?;
    if lookup.node(&note.id).is_some() {
        return Err(not_unique());
    }
    if lookup.node(&note.node_id).is_none() {
        return Err(invalid(
            "nodeId",
            "Note must be anchored to an existing node",
        ));
    }
    content(&note.text)?;
    author_of(&note.author)?;
    let limits = note_limits();
    if note.replies.len() > limits.replies {
        return Err(limits.too_many_replies());
    }
    for reply in &note.replies {
        content(&reply.text)?;
        author_of(&reply.author)?;
    }
    Ok(())
}

fn invalid(field: &str, message: &str) -> StorageError {
    StorageError::validation(field, message)
}

fn content(text: &str) -> Result<String> {
    note_limits().check_text(text)?;
    Ok(text.to_string())
}

fn author_of(author: &Option<String>) -> Result<Option<String>> {
    note_limits().check_author(author.as_deref())?;
    Ok(author.clone())
}
