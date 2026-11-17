//! `update_node`: assign or merge-patch one node's props (the `merge` rule of
//! `Candidate::apply`), keeping usage/link indexes and, when component status flips,
//! the subtree's master index current. A master losing its status while used is
//! checked at the end of the batch, as full validation would. No node is ever deeper
//! than the maximum (load and reparent keep that), so `Candidate`'s per-update depth
//! check always passes here.
use super::Txn;
use crate::checks;
use crate::props::{self, is_master};
use fagent_scene_input::{Result, StorageError};
use serde_json::Value;

impl Txn<'_> {
    pub(super) fn update(&mut self, id: &str, patch: &Value, merge: bool) -> Result<()> {
        checks::not_virtual(id, None)?;
        let patch = patch.as_object().ok_or_else(|| {
            StorageError::validation("props", "Update properties must be an object")
        })?;
        let key = checks::existing(self.store, id)?;
        let node = self.store.node(key);
        let next = props::updated(&node.props, patch, merge)?;
        let was_master = is_master(&node.kind, &node.props);
        let now_master = is_master(&node.kind, &next);
        self.store.set_props(key, next, &mut self.log);
        checks::instance_target(self.store, self.store.node(key))?;
        self.unmarked |= was_master && !now_master;
        if was_master != now_master {
            self.store.refresh(key, &mut self.log);
        }
        for name in patch.keys() {
            self.store.stamp_key(key, name, self.rev, &mut self.log);
        }
        self.touch(id, true);
        Ok(())
    }
}
