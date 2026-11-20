//! Sibling order `(sort_key, id)`, matching the stored `ORDER BY parent_id, sort_key, id`.
use super::{Key, Store};
use std::cmp::Ordering;
use std::sync::Arc;

impl Store {
    /// Compares two live nodes in sibling order.
    pub(crate) fn sibling_cmp(&self, a: Key, b: Key) -> Ordering {
        let (a, b) = (self.node(a), self.node(b));
        (a.sort_key, a.id.as_str()).cmp(&(b.sort_key, b.id.as_str()))
    }

    /// Binary search of `child` (by its current sort key and id) in a sibling list.
    fn position(&self, list: &[Key], child: Key) -> Result<usize, usize> {
        list.binary_search_by(|probe| self.sibling_cmp(*probe, child))
    }

    /// Mutable sibling list of `parent` (the page list for `None`), copied first when
    /// another version shares it.
    pub(super) fn list_mut(&mut self, parent: Option<Key>) -> &mut Vec<Key> {
        match parent {
            Some(key) => Arc::make_mut(&mut self.slot_mut(key).children),
            None => Arc::make_mut(&mut self.pages),
        }
    }

    /// Inserts `child` into the ordered list of `parent`.
    pub(super) fn insert_sorted(&mut self, parent: Option<Key>, child: Key) {
        let at = match self.position(self.children(parent), child) {
            Ok(_) => unreachable!("ids are unique, so sibling keys never tie"),
            Err(at) => at,
        };
        self.list_mut(parent).insert(at, child);
    }

    /// Removes `child` from the ordered list of `parent`.
    pub(super) fn remove_sorted(&mut self, parent: Option<Key>, child: Key) {
        let at = self
            .position(self.children(parent), child)
            .expect("child is linked under its parent");
        self.list_mut(parent).remove(at);
    }

    /// Sorts every sibling list after a bulk load.
    pub(crate) fn sort_all(&mut self) {
        let mut pages = std::mem::take(Arc::make_mut(&mut self.pages));
        pages.sort_by(|a, b| self.sibling_cmp(*a, *b));
        self.pages = Arc::new(pages);
        let parents: Vec<Key> = self
            .iter()
            .filter(|(_, slot)| slot.children.len() > 1)
            .map(|(key, _)| key)
            .collect();
        for key in parents {
            let mut children = std::mem::take(self.list_mut(Some(key)));
            children.sort_by(|a, b| self.sibling_cmp(*a, *b));
            *self.list_mut(Some(key)) = children;
        }
    }

    /// Sets each child's sort key of `parent` to its position, counting position `at`
    /// as taken by a node about to be linked there (`Candidate::place` numbering).
    /// Relative order is kept, so the list stays sorted. Changed children are pushed
    /// to `changed`.
    pub(crate) fn renumber(&mut self, parent: Option<Key>, at: usize, changed: &mut Vec<Key>) {
        let list = self.children(parent).to_vec();
        for (position, key) in list.into_iter().enumerate() {
            let wanted = position as i64 + i64::from(position >= at);
            if self.node(key).sort_key != wanted {
                self.set_sort_key(key, wanted);
                changed.push(key);
            }
        }
    }
}
