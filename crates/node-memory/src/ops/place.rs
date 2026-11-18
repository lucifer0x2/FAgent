//! Sibling placement exactly as `Candidate::place`: the siblings in `(sort_key, id)`
//! order (the stored children index, so no project scan), the node inserted at `index`
//! or appended, then every sort key renumbered to its position `0..n`. Siblings whose
//! sort key changes are touched; the old parent's list is not renumbered.
use super::Txn;
use crate::store::Key;
use fagent_scene_input::{validate::structural, Result};

impl Txn<'_> {
    /// The position a node placed under `parent` at `index` gets (append for `None`),
    /// or `Candidate`'s out-of-bounds error. The node itself must not be linked there.
    pub(super) fn check_index(
        &self,
        id: &str,
        parent: Option<Key>,
        index: Option<usize>,
    ) -> Result<usize> {
        let count = self.store.children(parent).len();
        match index.unwrap_or(count) {
            at if at > count => {
                let parent = parent.map(|p| self.store.node(p).id.as_str());
                Err(structural::index_out_of_bounds(id, at, parent, count))
            }
            at => Ok(at),
        }
    }

    /// Links the unlinked `key` under `parent` at `index` (append when `None`).
    pub(super) fn place(
        &mut self,
        key: Key,
        parent: Option<Key>,
        index: Option<usize>,
    ) -> Result<()> {
        let at = self.check_index(&self.store.node(key).id, parent, index)?;
        self.store.renumber(parent, at, &mut self.renumbered);
        if self.store.node(key).sort_key != at as i64 {
            self.store.set_sort_key(key, at as i64);
        }
        // Under the new keys the list is still sorted and `key` lands at `at`.
        self.store.link(parent, key);
        Ok(())
    }
}
