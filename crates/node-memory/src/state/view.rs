//! The state as a reader view: a published revision is an O(1) clone of the version
//! the writer kept, so publishing copies nothing and a reader never waits on writers.
use super::ProjectState;
use crate::store::Key;
use fagent_scene_input::read_scope::{ReadView, ViewSource};
use fagent_scene_input::{Node, Note};
use std::sync::Arc;

impl ProjectState {
    /// This version as an immutable reader view.
    pub fn view(&self) -> ReadView {
        ReadView::from_source(Arc::new(self.clone()), self.meta.clone())
    }

    fn sorted_ids(&self, keys: impl Iterator<Item = Key>) -> Vec<&str> {
        let mut ids: Vec<&str> = keys.map(|k| self.store.node(k).id.as_str()).collect();
        ids.sort_unstable();
        ids
    }
}

impl ViewSource for ProjectState {
    fn node_count(&self) -> usize {
        self.store.len()
    }

    fn node(&self, id: &str) -> Option<&Node> {
        ProjectState::node(self, id)
    }

    fn note(&self, id: &str) -> Option<&Note> {
        self.store.res.note(id)
    }

    fn children(&self, id: &str) -> Box<dyn Iterator<Item = &Node> + '_> {
        let keys = self
            .store
            .get(id)
            .map_or(&[][..], |slot| &slot.children[..]);
        Box::new(keys.iter().map(|&k| self.store.node(k)))
    }

    fn child_count(&self, id: &str) -> usize {
        self.store.get(id).map_or(0, |slot| slot.children.len())
    }

    fn roots(&self) -> Box<dyn Iterator<Item = &Node> + '_> {
        Box::new(self.pages())
    }

    fn nodes(&self) -> Box<dyn Iterator<Item = &Node> + '_> {
        Box::new(self.store.iter().map(|(_, slot)| slot.node.as_ref()))
    }

    fn notes(&self) -> Box<dyn Iterator<Item = &Note> + '_> {
        Box::new(ProjectState::notes(self))
    }

    fn page(&self, id: &str) -> Option<&str> {
        self.page_of(id).map(|page| page.id.as_str())
    }

    fn notes_on(&self, id: &str) -> Vec<&Note> {
        ProjectState::notes_on(self, id).collect()
    }

    fn component_users(&self, master: &str) -> Vec<&str> {
        self.sorted_ids(self.store.users(master))
    }

    fn linking(&self, target: &str) -> Vec<&str> {
        self.sorted_ids(self.store.linking(target))
    }

    fn reference_users(&self, key: &str) -> Vec<&str> {
        self.sorted_ids(self.store.referencing(key))
    }

    fn estimated_bytes(&self) -> usize {
        self.estimated_bytes()
    }
}
