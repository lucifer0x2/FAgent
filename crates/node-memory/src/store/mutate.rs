//! Store mutations. Logged ones push their exact inverse onto an undo log; `load_*`
//! ones build a fresh store in bulk and are never rolled back.
use super::{node_bytes, string_bytes, Derived, Key, Slot, Stamps, Store, Undo};
use fagent_scene_input::Node;
use serde_json::Value;

pub(crate) type Log = Vec<Undo>;

/// Derived values of a slot not yet indexed.
const PLACEHOLDER: Derived = Derived {
    page: Key(0),
    root: None,
    master: None,
};

/// Estimated bytes of a slot's heap data.
pub(super) fn slot_bytes(slot: &Slot) -> usize {
    node_bytes(&slot.node) + slot.stamps.bytes()
}

impl Store {
    pub(super) fn slot_mut(&mut self, key: Key) -> &mut Slot {
        self.slots[key.index()].as_mut().expect("live node key")
    }

    /// Allocates a slot (reusing a freed one first) without linking it.
    fn allocate(&mut self, slot: Slot) -> (Key, bool) {
        self.len += 1;
        self.bytes += slot_bytes(&slot);
        let (key, reused) = match self.free.pop() {
            Some(key) => (key, true),
            None => {
                let key = Key(u32::try_from(self.slots.len()).expect("slot count fits u32"));
                self.slots.push(None);
                (key, false)
            }
        };
        self.ids.insert(slot.node.id.clone(), key);
        self.index_refs(key, &slot.node);
        self.slots[key.index()] = Some(slot);
        (key, reused)
    }

    /// Adds a node whose `parent_id` already names `parent`; link it with
    /// [`Store::link`] and fill its indexes with [`Store::init`].
    pub(crate) fn insert(
        &mut self,
        node: Node,
        parent: Option<Key>,
        stamps: Stamps,
        log: &mut Log,
    ) -> Key {
        let slot = Slot {
            node,
            parent,
            children: Vec::new(),
            derived: PLACEHOLDER,
            stamps,
        };
        let (key, reused) = self.allocate(slot);
        log.push(Undo::Inserted { key, reused });
        key
    }

    /// Frees an already unlinked node; its slot moves into the log, never cloned.
    pub(crate) fn remove(&mut self, key: Key, log: &mut Log) {
        let slot = self.slots[key.index()].take().expect("live node key");
        self.ids.remove(slot.node.id.as_str());
        self.unindex_refs(key, &slot.node);
        self.len -= 1;
        self.bytes -= slot_bytes(&slot);
        self.free.push(key);
        log.push(Undo::Removed {
            key,
            slot: Box::new(slot),
        });
    }

    pub(crate) fn link(&mut self, parent: Option<Key>, child: Key, log: &mut Log) {
        self.insert_sorted(parent, child);
        log.push(Undo::Linked { parent, child });
    }

    pub(crate) fn unlink(&mut self, parent: Option<Key>, child: Key, log: &mut Log) {
        self.remove_sorted(parent, child);
        log.push(Undo::Unlinked { parent, child });
    }

    /// Sets the parent link and the stored `parent_id` together.
    pub(crate) fn set_parent(&mut self, key: Key, parent: Option<Key>, log: &mut Log) {
        let parent_id = parent.map(|p| self.node(p).id.clone());
        let slot = self.slot_mut(key);
        let old = std::mem::replace(&mut slot.parent, parent);
        let old_id = std::mem::replace(&mut slot.node.parent_id, parent_id);
        let added = slot.node.parent_id.as_deref().map_or(0, string_bytes);
        self.bytes = self.bytes + added - old_id.as_deref().map_or(0, string_bytes);
        log.push(Undo::Parent { key, old, old_id });
    }

    /// Sets a sort key in place. The caller keeps the sibling list ordered.
    pub(crate) fn set_sort_key(&mut self, key: Key, sort_key: i64, log: &mut Log) {
        let old = std::mem::replace(&mut self.slot_mut(key).node.sort_key, sort_key);
        log.push(Undo::SortKey { key, old });
    }

    pub(crate) fn set_props(&mut self, key: Key, props: Value, log: &mut Log) {
        let added = crate::props::heap_bytes(&props);
        let old = std::mem::replace(&mut self.slot_mut(key).node.props, props);
        self.bytes = self.bytes + added - crate::props::heap_bytes(&old);
        self.reindex_refs(key, &old);
        log.push(Undo::Props { key, old });
    }

    pub(crate) fn set_derived(&mut self, key: Key, derived: Derived, log: &mut Log) {
        let old = std::mem::replace(&mut self.slot_mut(key).derived, derived);
        log.push(Undo::Derived { key, old });
    }

    /// Bulk load: room for `additional` nodes, so the arena and id map grow once.
    pub(crate) fn reserve(&mut self, additional: usize) {
        self.slots.reserve_exact(additional);
        self.ids.reserve(additional);
    }

    /// Bulk load: adds a node with placeholder links and derived values.
    pub(crate) fn load_node(&mut self, node: Node) -> Key {
        let slot = Slot {
            node,
            parent: None,
            children: Vec::new(),
            derived: PLACEHOLDER,
            stamps: Stamps::default(),
        };
        self.allocate(slot).0
    }

    /// Bulk load: appends `child` unsorted; call `sort_all` once afterwards.
    pub(crate) fn load_link(&mut self, parent: Option<Key>, child: Key) {
        if let Some(p) = parent {
            self.slot_mut(child).parent = Some(p);
        }
        self.list_mut(parent).push(child);
    }

    /// Sets derived values of a slot that is being loaded or was inserted in the
    /// current log. Not logged: undoing that insert discards the whole slot.
    pub(crate) fn init(&mut self, key: Key, derived: Derived) {
        self.slot_mut(key).derived = derived;
    }
}
