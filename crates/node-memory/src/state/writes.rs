//! Last-writer-wins groundwork: which revision last wrote each property of a node
//! since the project was loaded. Writes at or before the load revision are unknown
//! here (`None`); a caller with an older base revision needs commit history instead.
use super::ProjectState;
use crate::store::Prop;

/// A stamped property of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Written<'a> {
    /// `(parentId, sortKey)` set explicitly by create/reparent/reorder. Siblings
    /// renumbered as a side effect are not stamped.
    Placement,
    /// The node was created (every key and the placement count as written).
    Created,
    /// A top-level props key (written by `update_node` or link pruning).
    Prop(&'a str),
}

impl ProjectState {
    /// Revision the project was loaded at; stamps exist only for later writes.
    pub fn loaded_revision(&self) -> u64 {
        self.loaded
    }

    fn absolute(&self, offset: u32) -> u64 {
        self.loaded + u64::from(offset)
    }

    /// Revision that last wrote top-level props key `prop` of node `id` after load,
    /// counting the node's creation. `None` when not written since load.
    pub fn prop_revision(&self, id: &str, prop: &str) -> Option<u64> {
        let slot = self.store.get(id)?;
        let prop = self.store.find_prop(prop).unwrap_or(Prop::Created);
        slot.stamps.get(prop).map(|r| self.absolute(r))
    }

    /// Revision that last placed node `id` after load, counting its creation.
    pub fn placement_revision(&self, id: &str) -> Option<u64> {
        let slot = self.store.get(id)?;
        slot.stamps.get(Prop::Placement).map(|r| self.absolute(r))
    }

    /// Every stamped property of node `id` with the revision that wrote it.
    pub fn writes(&self, id: &str) -> Vec<(Written<'_>, u64)> {
        let Some(slot) = self.store.get(id) else {
            return Vec::new();
        };
        let written = |prop: Prop| match prop {
            Prop::Placement => Written::Placement,
            Prop::Created => Written::Created,
            Prop::Key(_) => Written::Prop(self.store.prop_name(prop).expect("interned key")),
        };
        slot.stamps
            .iter()
            .map(|(prop, rev)| (written(prop), self.absolute(rev)))
            .collect()
    }
}

impl ProjectState {
    /// `(stamped nodes, stamp bytes)`; O(project), for benchmarks and diagnostics.
    #[doc(hidden)]
    pub fn stamp_stats(&self) -> (usize, usize) {
        let sizes = self.store.iter().map(|(_, slot)| slot.stamps.bytes());
        sizes
            .filter(|&b| b > 0)
            .fold((0, 0), |(n, total), b| (n + 1, total + b))
    }

    /// Bytes of one trie slot (each authored node holds one).
    #[doc(hidden)]
    pub fn slot_size() -> usize {
        crate::store::SLOT_SIZE
    }
}

impl ProjectState {
    /// Ids of nodes whose stored row, children or derived values differ from `base`
    /// (an earlier version of this project), found by pointer comparison: parts shared
    /// with `base` are skipped without being visited. For tests and diagnostics.
    #[doc(hidden)]
    pub fn changed_nodes_since(&self, base: &ProjectState) -> Vec<String> {
        let keys = self.store.changed_since(&base.store);
        let id = |k| {
            let slot = self.store.live(k).or_else(|| base.store.live(k));
            slot.map(|s| s.node.id.clone())
        };
        let mut ids: Vec<String> = keys.into_iter().filter_map(id).collect();
        ids.sort_unstable();
        ids
    }
}
