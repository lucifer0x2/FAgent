//! The rows a staged batch persists: only those that differ from the base revision.
use crate::{Delta, ProjectState};
use fagent_scene_input::read_scope::ReadView;
use fagent_scene_input::{Node, Note, Style, Token};
use std::collections::{BTreeSet, HashMap};

/// Rows of one batch as the staged state holds them. An upsert that leaves a row
/// equal to its base (an update that changed no value, a token written with its
/// current value) is not listed, though its id stays in the changed ids.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Rows {
    pub nodes: Vec<Node>,
    pub deleted_nodes: Vec<String>,
    pub tokens: Vec<Token>,
    pub styles: Vec<Style>,
    pub notes: Vec<Note>,
    pub deleted_notes: Vec<String>,
}

impl Rows {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
            && self.deleted_nodes.is_empty()
            && self.tokens.is_empty()
            && self.styles.is_empty()
            && self.notes.is_empty()
            && self.deleted_notes.is_empty()
    }
}

pub(super) fn build(state: &ProjectState, delta: &Delta, before: &ReadView) -> Rows {
    let nodes = delta
        .nodes
        .upserted
        .iter()
        .filter_map(|id| state.node(id))
        .filter(|node| !crate::props::same(before.node(&node.id), Some(node)))
        .cloned()
        .collect();
    let notes = delta
        .notes
        .upserted
        .iter()
        .filter_map(|id| state.note(id))
        .filter(|note| !crate::props::same(before.note(&note.id), Some(note)))
        .cloned()
        .collect();
    let meta = before.metadata();
    let tokens = changed(
        state.tokens(),
        &delta.tokens,
        meta.tokens.iter().map(|t| (t.id.as_str(), t)),
    );
    let styles = changed(
        state.styles(),
        &delta.styles,
        meta.styles.iter().map(|s| (s.id.as_str(), s)),
    );
    Rows {
        nodes,
        deleted_nodes: delta.nodes.deleted.clone(),
        tokens,
        styles,
        notes,
        deleted_notes: delta.notes.deleted.clone(),
    }
}

/// The rows of `after` named by `ids` that differ from the base rows. One pass over
/// each side, and only when some id was written.
fn changed<'a, T: PartialEq + Clone + HasId + serde::Serialize + 'a>(
    after: impl Iterator<Item = &'a T>,
    ids: &[String],
    base: impl Iterator<Item = (&'a str, &'a T)>,
) -> Vec<T> {
    if ids.is_empty() {
        return Vec::new();
    }
    let wanted: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
    let base: HashMap<&str, &T> = base.filter(|(id, _)| wanted.contains(id)).collect();
    after
        .filter(|row| wanted.contains(row.id()))
        .filter(|row| !crate::props::same(base.get(row.id()).copied(), Some(row)))
        .cloned()
        .collect()
}

trait HasId {
    fn id(&self) -> &str;
}

impl HasId for Token {
    fn id(&self) -> &str {
        &self.id
    }
}

impl HasId for Style {
    fn id(&self) -> &str {
        &self.id
    }
}
