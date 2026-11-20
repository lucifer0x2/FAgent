//! End-of-batch steps, after every operation applied: the node cap and link pruning
//! (as `Candidate::prune_links`).
use super::{Rejection, Txn};
use crate::checks;
use crate::props::pruned_links;
use crate::store::Key;
use std::collections::{BTreeSet, HashSet};

impl Txn<'_> {
    /// The node cap is checked here, on the final state and without an operation
    /// index, as `Candidate::validate` checks it first: a batch may pass the cap
    /// mid-way and delete back under it.
    pub(crate) fn finish(&mut self) -> std::result::Result<(), Rejection> {
        if self.store.len() > self.max_nodes {
            return Err(checks::too_many(self.max_nodes).into());
        }
        self.prune_links();
        Ok(())
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
}
