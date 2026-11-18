//! `reparent_node` and `reorder_node`: sibling-list moves with `Candidate::place`
//! numbering, plus a derived-index refresh limited to the moved subtree (and pruned
//! where values do not change).
use super::{Rejection, Txn};
use crate::checks;
use crate::store::Prop;
use fagent_scene_input::validate::check_node;
use fagent_scene_input::Result;

impl Txn<'_> {
    /// `Candidate::apply` order: the structural errors `check_node` gives first (made
    /// before the move so the store never holds a cycle), the move, `check_node`, then
    /// placement. Last comes a check `Candidate` leaves to full validation: moved
    /// deeper, the subtree's descendants must stay within the maximum depth. That one
    /// is [`Rejection::inexact`].
    pub(super) fn reparent(
        &mut self,
        id: &str,
        parent_id: &str,
        index: Option<usize>,
    ) -> std::result::Result<(), Rejection> {
        checks::not_virtual(id, Some(parent_id))?;
        let key = checks::existing(self.store, id)?;
        let kind = self.store.node(key).kind.clone();
        let parent = checks::parent(self.store, id, &kind, Some(parent_id))?
            .expect("a non-page has a parent");
        if self.store.ancestors(parent).any(|a| a == key) {
            return Err(checks::cycle(id, parent_id).into());
        }
        let depth = 1 + checks::depth_of(self.store, parent);
        checks::depth(depth)?;
        let deeper = depth > checks::depth_of(self.store, key);
        let old_parent = self.store.slot(key).parent;
        self.store.unlink(old_parent, key);
        if old_parent != Some(parent) {
            self.store.set_parent(key, Some(parent));
        }
        check_node(&*self.store, self.store.node(key))?;
        self.place(key, Some(parent), index)?;
        if deeper {
            let height = checks::height(self.store, key);
            checks::depth(depth + height).map_err(Rejection::inexact)?;
        }
        self.store.refresh(key);
        self.store.stamp(key, Prop::Placement, self.rev);
        self.touch(id, true);
        Ok(())
    }

    pub(super) fn reorder(&mut self, id: &str, index: usize) -> Result<()> {
        checks::not_virtual(id, None)?;
        let key = checks::existing(self.store, id)?;
        let parent = self.store.slot(key).parent;
        self.store.unlink(parent, key);
        self.place(key, parent, Some(index))?;
        self.store.stamp(key, Prop::Placement, self.rev);
        self.touch(id, true);
        Ok(())
    }
}
