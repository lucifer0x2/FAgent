//! Note operations with the checks, messages and order of `Candidate::apply_note`.
use super::Txn;
use crate::checks;
use fagent_scene_input::{note_limits, Note, NoteReply, Operation, Result, StorageError};

fn invalid(field: &str, message: &str) -> StorageError {
    StorageError::validation(field, message)
}

fn content(text: &str) -> Result<String> {
    note_limits().check_text(text)?;
    Ok(text.to_owned())
}

fn author_of(author: &Option<String>) -> Result<Option<String>> {
    note_limits().check_author(author.as_deref())?;
    Ok(author.clone())
}

impl Txn<'_> {
    fn anchor(&self, node: &str) -> Result<()> {
        match self.store.contains(node) {
            true => Ok(()),
            false => Err(invalid(
                "nodeId",
                "Note must be anchored to an existing node",
            )),
        }
    }

    fn existing_note(&self, id: &str) -> Result<Note> {
        let note = self.store.res.notes.get(id);
        note.map(|n| n.as_ref().clone())
            .ok_or_else(|| invalid("id", "Note does not exist"))
    }

    fn put(&mut self, id: &str, note: Option<Note>) {
        let existed = self.store.res.notes.contains_key(id);
        self.store.put_note(id, note);
        self.touch_note(id, existed);
    }

    pub(super) fn note(&mut self, op: &Operation) -> Result<()> {
        match op {
            Operation::CreateNote {
                id,
                node_id,
                text,
                author,
            } => {
                checks::valid(id)?;
                if self.store.res.notes.contains_key(id) {
                    return Err(invalid("id", "Note ID already exists"));
                }
                if self.store.contains(id) {
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
                self.put(id, Some(note));
            }
            Operation::ReplyNote { id, text, author } => {
                let reply = NoteReply {
                    text: content(text)?,
                    author: author_of(author)?,
                };
                let mut note = self.existing_note(id)?;
                let limits = note_limits();
                if note.replies.len() >= limits.replies {
                    return Err(limits.replies_full());
                }
                note.replies.push(reply);
                self.put(id, Some(note));
            }
            Operation::UpdateNote {
                id,
                text,
                resolved,
                node_id,
            } => {
                let mut note = self.existing_note(id)?;
                if text.is_none() && resolved.is_none() && node_id.is_none() {
                    return Err(invalid("update_note", "Set text, resolved or nodeId"));
                }
                if let Some(node) = node_id {
                    self.anchor(node)?;
                }
                if let Some(text) = text.as_deref().map(content).transpose()? {
                    note.text = text;
                }
                if let Some(resolved) = resolved {
                    note.resolved = *resolved;
                }
                if let Some(node) = node_id {
                    note.node_id = node.clone();
                }
                self.put(id, Some(note));
            }
            Operation::DeleteNote { id } => {
                self.existing_note(id)?;
                self.put(id, None);
            }
            _ => return Err(invalid("op", "Not a note operation")),
        }
        Ok(())
    }
}
