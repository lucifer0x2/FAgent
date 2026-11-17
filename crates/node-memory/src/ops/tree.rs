//! `reparent_node` and `reorder_node`: sibling-list moves with `Candidate::place`
//! numbering, plus a derived-index refresh limited to the moved subtree (and pruned
//! where values do not change).
use super::Txn;
use crate::checks;
use crate::store::Prop;
use fagent_scene_input::Result;

impl Txn<'_> {
    pub(super) fn reparent(
        &mut self,
        id: &str,
        parent_id: &str,
        index: Option<usize>,
    ) -> Result<()> {
        checks::not_virtual(id, Some(parent_id))?;
        let key = checks::existing(self.store, id)?;
        let kind = self.store.node(key).kind.clone();
        let parent = checks::parent(self.store, id, &kind, Some(parent_id))?
            .expect("a non-page has a parent");
        if self.store.ancestors(parent).any(|a| a == key) {
            return Err(checks::cycle(id, parent_id));
        }
        let depth = 1 + checks::depth_of(self.store, parent);
        checks::depth(depth)?;
        if depth > checks::depth_of(self.store, key) {
            // Moving deeper: descendants must stay within the maximum too.
            checks::depth(depth + checks::height(self.store, key))?;
        }
        checks::instance_target(self.store, self.store.node(key))?;
        let old_parent = self.store.slot(key).parent;
        self.store.unlink(old_parent, key, &mut self.log);
        if old_parent != Some(parent) {
            self.store.set_parent(key, Some(parent), &mut self.log);
        }
        self.place(key, Some(parent), index)?;
        self.store.refresh(key, &mut self.log);
        self.store
            .stamp(key, Prop::Placement, self.rev, &mut self.log);
        self.touch(id, true);
        Ok(())
    }

    pub(super) fn reorder(&mut self, id: &str, index: usize) -> Result<()> {
        checks::not_virtual(id, None)?;
        let key = checks::existing(self.store, id)?;
        let parent = self.store.slot(key).parent;
        self.store.unlink(parent, key, &mut self.log);
        self.place(key, parent, Some(index))?;
        self.store
            .stamp(key, Prop::Placement, self.rev, &mut self.log);
        self.touch(id, true);
        Ok(())
    }
}
