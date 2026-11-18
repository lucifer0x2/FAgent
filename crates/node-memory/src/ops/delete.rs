//! `delete_subtree`: O(subtree) removal. Fails with `COMPONENT_IN_USE` when an
//! instance outside the subtree uses any removed id (`component_delete_guard`); notes
//! anchored to removed nodes are deleted with them (`cascade_notes`).
use super::Txn;
use crate::checks;
use crate::store::Key;
use fagent_scene_input::Result;
use std::collections::HashSet;

impl Txn<'_> {
    pub(super) fn delete(&mut self, id: &str) -> Result<()> {
        checks::not_virtual(id, None)?;
        let root = checks::existing(self.store, id)?;
        let removed = self.store.subtree_keys(root);
        let store = &*self.store;
        let users: Vec<Key> = removed
            .iter()
            .flat_map(|&k| store.users(&store.node(k).id))
            .collect();
        if !users.is_empty() {
            let inside: HashSet<Key> = removed.iter().copied().collect();
            let outside: Vec<String> = users
                .into_iter()
                .filter(|user| !inside.contains(user))
                .map(|user| store.node(user).id.clone())
                .collect();
            if !outside.is_empty() {
                return Err(checks::component_in_use(outside));
            }
        }
        let notes: Vec<String> = removed
            .iter()
            .flat_map(|&k| store.res.notes_on(&store.node(k).id))
            .map(str::to_owned)
            .collect();
        for note in notes {
            self.store.put_note(&note, None);
            self.touch_note(&note, true);
        }
        let parent = self.store.slot(root).parent;
        self.store.unlink(parent, root);
        for &key in removed.iter().rev() {
            let id = self.store.node(key).id.clone();
            self.store.remove(key);
            self.touch_owned(id, true);
        }
        Ok(())
    }
}
