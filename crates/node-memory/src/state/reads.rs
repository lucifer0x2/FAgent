//! Borrowing reads. Every lookup is O(1) by id plus the size of what it returns.
use super::{ProjectState, Subtree};
use crate::store::Key;
use fagent_scene_input::{Node, Note, Style, Token};

impl ProjectState {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.store.get(id).map(|slot| &slot.node)
    }

    /// Pages in sibling order `(sort_key, id)`.
    pub fn pages(&self) -> impl ExactSizeIterator<Item = &Node> + '_ {
        self.nodes(self.store.children(None))
    }

    /// Direct children in sibling order `(sort_key, id)`; `None` for an unknown id.
    pub fn children(&self, id: &str) -> Option<impl ExactSizeIterator<Item = &Node> + '_> {
        let slot = self.store.get(id)?;
        Some(self.nodes(&slot.children))
    }

    /// `id` and its descendants in preorder, children in sibling order.
    pub fn subtree(&self, id: &str) -> Option<Subtree<'_>> {
        self.store.key(id).map(|key| Subtree::new(&self.store, key))
    }

    /// The page containing `id` (itself for a page).
    pub fn page_of(&self, id: &str) -> Option<&Node> {
        let slot = self.store.get(id)?;
        Some(self.store.node(slot.derived.page))
    }

    /// The ancestor-or-self directly under the page; `None` for pages.
    pub fn root_frame_of(&self, id: &str) -> Option<&Node> {
        self.store.get(id)?.derived.root.map(|k| self.store.node(k))
    }

    /// The outermost component master that is `id` or contains it.
    pub fn master_of(&self, id: &str) -> Option<&Node> {
        self.store
            .get(id)?
            .derived
            .master
            .map(|k| self.store.node(k))
    }

    /// Instances whose `props.componentId` is `component_id`, in no defined order.
    pub fn instances_of<'a>(&'a self, component_id: &'a str) -> impl Iterator<Item = &'a Node> {
        let users = self.store.users(component_id);
        users.map(|k| self.store.node(k))
    }

    /// Nodes with a `props.links` entry whose `to` is `id`, in no defined order.
    pub fn linking_to<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a Node> {
        self.store.linking(id).map(|k| self.store.node(k))
    }

    /// The snapshot resources, by id.
    pub fn tokens(&self) -> impl ExactSizeIterator<Item = &Token> + '_ {
        self.store.res.tokens.values()
    }

    pub fn styles(&self) -> impl ExactSizeIterator<Item = &Style> + '_ {
        self.store.res.styles.values()
    }

    pub fn notes(&self) -> impl ExactSizeIterator<Item = &Note> + '_ {
        self.store.res.notes.values()
    }

    pub fn note(&self, id: &str) -> Option<&Note> {
        self.store.res.notes.get(id)
    }

    /// Notes anchored to node `id`, by note id.
    pub fn notes_on<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a Note> {
        let ids = self.store.res.notes_on(id);
        ids.map(|note| &self.store.res.notes[note])
    }

    fn nodes<'a>(&'a self, keys: &'a [Key]) -> impl ExactSizeIterator<Item = &'a Node> + 'a {
        keys.iter().map(|k| self.store.node(*k))
    }
}
