//! Sibling order `(sort_key, id)`, matching the stored `ORDER BY parent_id, sort_key, id`.
use super::{Key, Log, Store, Undo};
use std::cmp::Ordering;

impl Store {
    /// Compares two live nodes in sibling order.
    pub(crate) fn sibling_cmp(&self, a: Key, b: Key) -> Ordering {
        let (a, b) = (self.node(a), self.node(b));
        (a.sort_key, a.id.as_str()).cmp(&(b.sort_key, b.id.as_str()))
    }

    /// Binary search of `child` (by its current sort key and id) in a sibling list.
    pub(super) fn position(&self, list: &[Key], child: Key) -> Result<usize, usize> {
        list.binary_search_by(|probe| self.sibling_cmp(*probe, child))
    }

    /// Mutable sibling list of `parent` (the page list for `None`).
    pub(super) fn list_mut(&mut self, parent: Option<Key>) -> &mut Vec<Key> {
        match parent {
            Some(key) => {
                &mut self.slots[key.index()]
                    .as_mut()
                    .expect("live node key")
                    .children
            }
            None => &mut self.pages,
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
        let mut pages = std::mem::take(&mut self.pages);
        pages.sort_by(|a, b| self.sibling_cmp(*a, *b));
        self.pages = pages;
        for i in 0..self.slots.len() {
            let Some(mut children) = self.slots[i]
                .as_mut()
                .map(|s| std::mem::take(&mut s.children))
            else {
                continue;
            };
            children.sort_by(|a, b| self.sibling_cmp(*a, *b));
            self.slots[i].as_mut().expect("live").children = children;
        }
    }
}

impl Store {
    /// Sets each child's sort key of `parent` to its position, counting position `at`
    /// as taken by a node about to be linked there (`Candidate::place` numbering).
    /// Relative order is kept, so the list stays sorted. Changed children are pushed
    /// to `changed`; one log entry holds the old keys when any changed.
    pub(crate) fn renumber(
        &mut self,
        parent: Option<Key>,
        at: usize,
        changed: &mut Vec<Key>,
        log: &mut Log,
    ) {
        let list = std::mem::take(self.list_mut(parent));
        let mut old: Option<Vec<i64>> = None;
        for (position, &key) in list.iter().enumerate() {
            let wanted = position as i64 + i64::from(position >= at);
            if self.node(key).sort_key == wanted {
                continue;
            }
            if old.is_none() {
                old = Some(list.iter().map(|k| self.node(*k).sort_key).collect());
            }
            self.slot_mut(key).node.sort_key = wanted;
            changed.push(key);
        }
        *self.list_mut(parent) = list;
        if let Some(old) = old {
            log.push(Undo::Renumber { parent, old });
        }
    }

    /// Restores the sort keys of `parent`'s children, by position.
    pub(super) fn unrenumber(&mut self, parent: Option<Key>, old: Vec<i64>) {
        let list = std::mem::take(self.list_mut(parent));
        for (&key, sort_key) in list.iter().zip(old) {
            self.slot_mut(key).node.sort_key = sort_key;
        }
        *self.list_mut(parent) = list;
    }
}
