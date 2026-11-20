//! Store mutations. Each writes on this version only: shared trie paths, nodes and
//! sibling lists are copied on first write (`Arc::make_mut`), so earlier versions
//! never change. `load_*` ones build a fresh store in bulk.
use super::{node_bytes, string_bytes, Derived, Key, Slot, Stamps, Store};
use fagent_scene_input::Node;
use serde_json::Value;
use std::sync::Arc;

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
        self.slots.get_mut(key.0).expect("live node key")
    }

    /// The node of `key` for writing; copied first when another version shares it.
    pub(super) fn node_mut(&mut self, key: Key) -> &mut Node {
        Arc::make_mut(&mut self.slot_mut(key).node)
    }

    /// Adds a slot under its interned id without linking it.
    fn allocate(&mut self, slot: Slot) -> Key {
        let key = Key(self.ids.intern(&slot.node.id));
        self.bytes += slot_bytes(&slot);
        let node = slot.node.clone();
        let old = self.slots.insert(key.0, slot);
        debug_assert!(old.is_none(), "ids are unique");
        self.index_refs(key, &node);
        key
    }

    /// Adds a node whose `parent_id` already names `parent`; link it with
    /// [`Store::link`] and fill its indexes with [`Store::init`].
    pub(crate) fn insert(&mut self, node: Node, parent: Option<Key>, stamps: Stamps) -> Key {
        self.allocate(Slot {
            node: Arc::new(node),
            parent,
            children: Arc::default(),
            derived: PLACEHOLDER,
            stamps,
        })
    }

    /// Drops an already unlinked node.
    pub(crate) fn remove(&mut self, key: Key) {
        let slot = self.slots.remove(key.0).expect("live node key");
        self.unindex_refs(key, &slot.node);
        self.bytes -= slot_bytes(&slot);
    }

    pub(crate) fn link(&mut self, parent: Option<Key>, child: Key) {
        self.insert_sorted(parent, child);
    }

    pub(crate) fn unlink(&mut self, parent: Option<Key>, child: Key) {
        self.remove_sorted(parent, child);
    }

    /// Sets the parent link and the stored `parent_id` together.
    pub(crate) fn set_parent(&mut self, key: Key, parent: Option<Key>) {
        let parent_id = parent.map(|p| self.node(p).id.clone());
        let added = parent_id.as_deref().map_or(0, string_bytes);
        self.slot_mut(key).parent = parent;
        let old = std::mem::replace(&mut self.node_mut(key).parent_id, parent_id);
        self.bytes = self.bytes + added - old.as_deref().map_or(0, string_bytes);
    }

    /// Sets a sort key. The caller keeps the sibling list ordered.
    pub(crate) fn set_sort_key(&mut self, key: Key, sort_key: i64) {
        self.node_mut(key).sort_key = sort_key;
    }

    pub(crate) fn set_props(&mut self, key: Key, props: Value) {
        let added = crate::props::heap_bytes(&props);
        let old = std::mem::replace(&mut self.node_mut(key).props, props);
        self.bytes = self.bytes + added - crate::props::heap_bytes(&old);
        self.reindex_refs(key, &old);
    }

    pub(crate) fn set_derived(&mut self, key: Key, derived: Derived) {
        self.slot_mut(key).derived = derived;
    }

    /// Bulk load: room for `additional` nodes in the id interner.
    pub(crate) fn reserve(&mut self, additional: usize) {
        self.ids.reserve(additional);
    }

    /// Bulk load: adds a node with placeholder links and derived values.
    pub(crate) fn load_node(&mut self, node: Node) -> Key {
        self.allocate(Slot {
            node: Arc::new(node),
            parent: None,
            children: Arc::default(),
            derived: PLACEHOLDER,
            stamps: Stamps::default(),
        })
    }

    /// Bulk load: appends `child` unsorted; call `sort_all` once afterwards.
    pub(crate) fn load_link(&mut self, parent: Option<Key>, child: Key) {
        if let Some(p) = parent {
            self.slot_mut(child).parent = Some(p);
        }
        self.list_mut(parent).push(child);
    }

    /// Sets derived values of a slot that is being loaded or was inserted by the
    /// current batch.
    pub(crate) fn init(&mut self, key: Key, derived: Derived) {
        self.slot_mut(key).derived = derived;
    }
}
