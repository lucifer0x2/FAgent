//! `update_node`: assign or merge-patch one node's props (the `merge` rule of
//! `Candidate::apply`), check the node as `Candidate` does after the write
//! (`check_node` on the working state), and keep usage/link indexes and, when
//! component status flips, the subtree's master index current. A master losing its
//! status while used is not an operation error: `Candidate` finds it in full
//! validation, as an instance whose `componentId` names no component frame.
use super::Txn;
use crate::checks;
use crate::props::{self, is_master};
use fagent_scene_input::validate::check_node;
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
        // Only a key whose value changes is stamped; an update that changes nothing
        // leaves props and stamps alone but still counts as a changed id.
        let written: Vec<bool> = patch
            .keys()
            .map(|name| !props::same(next.get(name), node.props.get(name)))
            .collect();
        let was_master = is_master(&node.kind, &node.props);
        let now_master = is_master(&node.kind, &next);
        if !props::same(Some(&next), Some(&node.props)) {
            self.store.set_props(key, next);
        }
        check_node(&*self.store, self.store.node(key))?;
        if was_master != now_master {
            self.store.refresh(key);
        }
        for (name, written) in patch.keys().zip(written) {
            if written {
                self.store.stamp_key(key, name, self.rev);
            }
        }
        self.touch(id, true);
        Ok(())
    }
}
