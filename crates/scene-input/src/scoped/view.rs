//! Immutable whole-project read payloads, shared between committed revisions.
use super::{persistent::Map, view_indexes::ViewIndexes, ScopedSnapshot};
use crate::{Node, Note, Snapshot};
use std::sync::{Arc, OnceLock};

/// A published revision. Nodes/notes and index shards use copy-on-write; keeping an
/// old Arc never keeps the writer locked and never exposes a staged mutation.
#[derive(Clone)]
pub struct ReadView {
    pub(super) snapshot: Arc<Snapshot>,
    pub(super) nodes: Map<Arc<Node>>,
    pub(super) notes: Map<Arc<Note>>,
    pub(super) index: Arc<ViewIndexes>,
    pub(super) resource_bytes: usize,
    /// Validation certificate belongs to this exact immutable revision, never a
    /// structure-only load. Successors receive one only after delta validation.
    pub(crate) validation: OnceLock<crate::validate::memory::Certificate>,
}

impl ReadView {
    /// The storage adapter must supply builtin snapshot source order on load.
    pub fn new(mut snapshot: Snapshot) -> Self {
        let mut nodes = Map::default();
        let mut notes = Map::default();
        for node in std::mem::take(&mut snapshot.nodes) {
            nodes.insert(node.id.clone(), Arc::new(node));
        }
        for note in std::mem::take(&mut snapshot.notes) {
            notes.insert(note.id.clone(), Arc::new(note));
        }
        let resource_bytes = resource_bytes(&snapshot);
        let mut view = Self {
            snapshot: Arc::new(snapshot),
            nodes,
            notes,
            index: Arc::default(),
            resource_bytes,
            validation: OnceLock::new(),
        };
        view.index = Arc::new(ViewIndexes::new(&view));
        view
    }

    /// Header and resource vectors only. Authored node/note access goes through the
    /// view; this is deliberately not a serialized/full snapshot representation.
    pub fn metadata(&self) -> &Arc<Snapshot> {
        &self.snapshot
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id).map(Arc::as_ref)
    }

    pub fn note(&self, id: &str) -> Option<&Note> {
        self.notes.get(id).map(Arc::as_ref)
    }

    pub fn children(&self, id: &str) -> impl Iterator<Item = &str> {
        self.index
            .children
            .get(id)
            .into_iter()
            .flat_map(|s| s.iter())
            .map(|(_, id)| id.as_str())
    }

    pub fn notes_on(&self, id: &str) -> impl Iterator<Item = &Note> {
        self.index
            .anchors
            .get(id)
            .into_iter()
            .flat_map(|s| s.iter())
            .filter_map(|id| self.note(id))
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn page(&self, id: &str) -> Option<&str> {
        self.index.pages.get(id).map(String::as_str)
    }

    pub fn reference_users<'a>(&'a self, key: &str) -> impl Iterator<Item = &'a str> {
        self.index
            .references
            .get(key)
            .into_iter()
            .flat_map(|s| s.iter())
            .map(String::as_str)
    }

    pub fn component_users(&self, master: &str) -> impl Iterator<Item = &str> {
        self.index
            .users
            .get(master)
            .into_iter()
            .flat_map(|s| s.iter())
            .map(String::as_str)
    }

    /// Explicit full representation for APIs outside the scoped-reader path.
    /// Normal memory reads never call this compatibility conversion.
    pub fn to_snapshot(&self) -> Snapshot {
        let mut snapshot = self.snapshot.as_ref().clone();
        snapshot.nodes = self
            .ordered(self.nodes.iter().map(|(id, _)| id))
            .into_iter()
            .cloned()
            .collect();
        snapshot.notes = self
            .notes
            .iter()
            .map(|(_, note)| note.as_ref().clone())
            .collect();
        snapshot.notes.sort_by(|a, b| a.id.cmp(&b.id));
        snapshot
    }

    /// Trusted adapter boundary, equivalent to `ScopedSnapshot::from_validated`.
    /// The adapter guarantees the entire authored revision passed validation.
    pub fn scoped(&self) -> ScopedSnapshot<'_> {
        ScopedSnapshot::from_view(self)
    }

    /// Current retained view proxy, not a measured allocator/RSS result. Old views
    /// retained by in-flight callers disappear when those callers release them.
    pub fn estimated_bytes(&self) -> usize {
        self.nodes.estimated_bytes()
            + self.notes.estimated_bytes()
            + self.index.estimated_bytes()
            + self.resource_bytes
            + self.validation.get().map_or(0, |v| v.estimated_bytes())
    }

    pub(super) fn ordered<'a>(
        &'a self,
        ids: impl IntoIterator<Item = &'a String>,
    ) -> Vec<&'a Node> {
        let mut nodes: Vec<_> = ids.into_iter().filter_map(|id| self.node(id)).collect();
        nodes.sort_by(|a, b| {
            (&a.parent_id, a.sort_key, &a.id).cmp(&(&b.parent_id, b.sort_key, &b.id))
        });
        nodes
    }
}

pub(super) fn node_bytes(node: &Node) -> usize {
    128 + node.id.len()
        + node.parent_id.as_ref().map_or(0, String::len)
        + node.props.to_string().len()
}

pub(super) fn note_bytes(note: &Note) -> usize {
    128 + serde_json::to_vec(note).map_or(0, |v| v.len())
}

pub(super) fn resource_bytes(snapshot: &Snapshot) -> usize {
    // Includes map roots/shard pointers even on an empty project.
    32 * 1024 + serde_json::to_vec(snapshot).map_or(0, |v| v.len())
}
