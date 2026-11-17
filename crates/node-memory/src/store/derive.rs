//! Rules for the ancestor-derived indexes (page, root frame, master) and their
//! incremental refresh, which visits only the part of a subtree whose values change.
use super::{Derived, Key, Log, Store};
use crate::props::is_master;
use fagent_scene_input::NodeKind;

impl Store {
    /// Derived values of `key` from its parent's current values.
    pub(crate) fn derive(&self, key: Key) -> Derived {
        let slot = self.slot(key);
        let Some(parent) = slot.parent else {
            return Derived {
                page: key,
                root: None,
                master: None,
            };
        };
        let above = self.slot(parent);
        let own_master = is_master(&slot.node.kind, &slot.node.props).then_some(key);
        Derived {
            page: above.derived.page,
            root: match above.node.kind {
                NodeKind::Page => Some(key),
                _ => above.derived.root,
            },
            master: above.derived.master.or(own_master),
        }
    }

    /// Recomputes `key` and descendants top-down. A node whose values are unchanged
    /// keeps its whole subtree unchanged, so the walk stops there.
    pub(crate) fn refresh(&mut self, key: Key, log: &mut Log) {
        let mut stack = vec![key];
        while let Some(key) = stack.pop() {
            let derived = self.derive(key);
            if derived == self.slot(key).derived {
                continue;
            }
            self.set_derived(key, derived, log);
            stack.extend_from_slice(&self.slot(key).children);
        }
    }
}
