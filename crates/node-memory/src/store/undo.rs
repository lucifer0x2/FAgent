//! Inverse of each logged mutation. Rolling a log back in reverse order restores the
//! store exactly: slots, free list, id map, sibling order, reference indexes, stamps,
//! resources and byte count.
use super::mutate::slot_bytes;
use super::{string_bytes, Derived, Key, Prop, ResUndo, Slot, Store};
use serde_json::Value;

#[derive(Debug)]
pub(crate) enum Undo {
    Inserted {
        key: Key,
        reused: bool,
    },
    Removed {
        key: Key,
        slot: Box<Slot>,
    },
    Linked {
        parent: Option<Key>,
        child: Key,
    },
    Unlinked {
        parent: Option<Key>,
        child: Key,
    },
    Parent {
        key: Key,
        old: Option<Key>,
        old_id: Option<String>,
    },
    SortKey {
        key: Key,
        old: i64,
    },
    /// Old sort keys of every child of `parent`, by position.
    Renumber {
        parent: Option<Key>,
        old: Vec<i64>,
    },
    Props {
        key: Key,
        old: Value,
    },
    Derived {
        key: Key,
        old: Derived,
    },
    Stamp {
        key: Key,
        prop: Prop,
        old: Option<u32>,
    },
    Res(ResUndo),
}

impl Store {
    /// Undoes every entry of `log`, newest first.
    pub(crate) fn rollback(&mut self, log: Vec<Undo>) {
        for entry in log.into_iter().rev() {
            self.undo(entry);
        }
    }

    fn undo(&mut self, entry: Undo) {
        match entry {
            Undo::Inserted { key, reused } => {
                let slot = self.slots[key.index()].take().expect("inserted slot");
                self.ids.remove(slot.node.id.as_str());
                self.unindex_refs(key, &slot.node);
                self.len -= 1;
                self.bytes -= slot_bytes(&slot);
                match reused {
                    true => self.free.push(key),
                    false => {
                        self.slots.pop();
                    }
                }
            }
            Undo::Removed { key, slot } => {
                let freed = self.free.pop();
                debug_assert_eq!(freed, Some(key), "free list unwinds in order");
                self.ids.insert(slot.node.id.clone(), key);
                self.index_refs(key, &slot.node);
                self.len += 1;
                self.bytes += slot_bytes(&slot);
                self.slots[key.index()] = Some(*slot);
            }
            Undo::Linked { parent, child } => self.remove_sorted(parent, child),
            Undo::Unlinked { parent, child } => self.insert_sorted(parent, child),
            Undo::Parent { key, old, old_id } => {
                let added = old_id.as_deref().map_or(0, string_bytes);
                let slot = self.slot_mut(key);
                slot.parent = old;
                let removed = std::mem::replace(&mut slot.node.parent_id, old_id);
                self.bytes = self.bytes + added - removed.as_deref().map_or(0, string_bytes);
            }
            Undo::SortKey { key, old } => self.slot_mut(key).node.sort_key = old,
            Undo::Renumber { parent, old } => self.unrenumber(parent, old),
            Undo::Props { key, old } => {
                let removed = crate::props::heap_bytes(&self.node(key).props);
                self.bytes = self.bytes + crate::props::heap_bytes(&old) - removed;
                let current = std::mem::replace(&mut self.slot_mut(key).node.props, old);
                self.reindex_refs(key, &current);
            }
            Undo::Derived { key, old } => self.slot_mut(key).derived = old,
            Undo::Stamp { key, prop, old } => self.unstamp(key, prop, old),
            Undo::Res(entry) => self.res.undo(entry),
        }
    }
}
