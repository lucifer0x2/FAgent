//! End-of-batch steps, after every operation applied: link pruning (as
//! `Candidate::prune_links`) and the checks full validation would make on the final
//! state rather than per operation.
use super::{Rejection, Txn};
use crate::checks;
use crate::props::{is_master, pruned_links};
use crate::store::Key;
use fagent_scene_input::Result;
use std::collections::{BTreeSet, HashSet};

impl Txn<'_> {
    /// The node cap is checked here, on the final state and without an operation
    /// index, as `Candidate::validate` does: a batch may pass the cap mid-way and
    /// delete back under it.
    /// Every error here is [`Rejection::inexact`]: `Candidate` finds these during full
    /// validation, in its own order, and the engine cap may be below its cap.
    pub(crate) fn finish(&mut self) -> std::result::Result<(), Rejection> {
        if self.store.len() > self.max_nodes {
            return Err(Rejection::inexact(checks::too_many(self.max_nodes)));
        }
        self.prune_links();
        self.check_masters().map_err(Rejection::inexact)
    }

    /// Drops links to nodes the batch deleted and did not recreate; an emptied `links`
    /// key is removed. Only nodes indexed as linking to a gone id are visited.
    fn prune_links(&mut self) {
        // Every touched node id that is not live was deleted by the batch; only those
        // still named by some node's links matter.
        let gone: HashSet<String> = self
            .nodes
            .keys()
            .filter(|id| !self.store.contains(id) && self.store.linking(id).next().is_some())
            .cloned()
            .collect();
        let sources: BTreeSet<Key> = gone.iter().flat_map(|id| self.store.linking(id)).collect();
        for key in sources {
            let props = &self.store.node(key).props;
            let Some(next) = pruned_links(props, |to| gone.contains(to)) else {
                continue;
            };
            self.store.set_props(key, next);
            self.store.stamp_key(key, "links", self.rev);
            self.touch_key(key);
        }
    }

    /// A node the batch touched that is no longer a component master must have no
    /// instances left (an earlier `update_node` removed `component`).
    fn check_masters(&self) -> Result<()> {
        if !self.unmarked {
            return Ok(());
        }
        let mut ids: Vec<&String> = self.nodes.keys().collect();
        ids.sort_unstable();
        for id in ids {
            let Some(slot) = self.store.get(id) else {
                continue;
            };
            if is_master(&slot.node.kind, &slot.node.props) || !self.store.has_users(id) {
                continue;
            }
            let users = self.store.users(id);
            let users = users.map(|u| self.store.node(u).id.clone()).collect();
            return Err(checks::component_in_use(users));
        }
        Ok(())
    }
}
