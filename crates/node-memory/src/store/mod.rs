//! Slot arena of nodes plus every structural index, keyed by compact `u32` slots.
//! Reads live here; logged mutations in `mutate`, their inverses in `undo`, the
//! props-derived reference indexes in `refs`, resources in `resources` and per-property
//! write revisions in `stamps`.
mod derive;
mod mutate;
mod order;
mod refs;
mod resources;
mod stamps;
mod undo;

use crate::props;
use fagent_scene_input::Node;
pub(crate) use mutate::Log;
pub(crate) use resources::{ResUndo, Resources};
pub(crate) use stamps::{Prop, Stamps};
use std::collections::{BTreeSet, HashMap};
pub(crate) use undo::Undo;

/// Compact node handle: an index into the slot arena. Reused after delete.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub(crate) struct Key(u32);

impl Key {
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

/// Ancestor-derived indexes of one node, a pure function of its ancestors' values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Derived {
    /// The page containing the node (the node itself for a page).
    pub page: Key,
    /// Ancestor-or-self directly under the page; `None` for pages.
    pub root: Option<Key>,
    /// Outermost component master that is the node or one of its ancestors.
    pub master: Option<Key>,
}

/// One live node with its links and derived indexes.
#[derive(Clone, Debug)]
pub(crate) struct Slot {
    pub node: Node,
    pub parent: Option<Key>,
    /// Children ordered by `(sort_key, id)`, the stored sibling order.
    pub children: Vec<Key>,
    pub derived: Derived,
    /// Revisions of writes since load, per property; empty until the first write.
    pub stamps: Stamps,
}

/// Bytes of one arena slot.
pub(crate) const SLOT_SIZE: usize = std::mem::size_of::<Option<Slot>>();

#[derive(Default)]
pub(crate) struct Store {
    slots: Vec<Option<Slot>>,
    free: Vec<Key>,
    ids: HashMap<String, Key>,
    /// Pages (children of no parent), ordered like any sibling list.
    pages: Vec<Key>,
    /// Instances by their `props.componentId` (the id string, live master or not).
    users: HashMap<String, BTreeSet<Key>>,
    /// Nodes by the ids their `props.links[].to` name.
    links: HashMap<String, BTreeSet<Key>>,
    /// Interned top-level props keys for stamps (never shrinks; not in `bytes`).
    props: stamps::Interner,
    pub(crate) res: Resources,
    len: usize,
    bytes: usize,
}

impl Store {
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn bytes(&self) -> usize {
        self.bytes + self.slots.len() * std::mem::size_of::<Option<Slot>>()
    }

    /// Estimated node bytes without the slot arena.
    pub(crate) fn content_bytes(&self) -> usize {
        self.bytes
    }

    pub(crate) fn key(&self, id: &str) -> Option<Key> {
        self.ids.get(id).copied()
    }

    pub(crate) fn contains(&self, id: &str) -> bool {
        self.ids.contains_key(id)
    }

    /// The slot of a live key. Keys handed out by this store are always live
    /// until removed, so a dead key is an internal invariant violation.
    pub(crate) fn slot(&self, key: Key) -> &Slot {
        self.slots[key.index()].as_ref().expect("live node key")
    }

    /// The slot of `key` when it is live (keys of deleted nodes may be reused).
    pub(crate) fn live(&self, key: Key) -> Option<&Slot> {
        self.slots.get(key.index()).and_then(Option::as_ref)
    }

    pub(crate) fn get(&self, id: &str) -> Option<&Slot> {
        self.key(id).map(|key| self.slot(key))
    }

    pub(crate) fn node(&self, key: Key) -> &Node {
        &self.slot(key).node
    }

    /// Ordered children of `parent`, or the pages for `None`.
    pub(crate) fn children(&self, parent: Option<Key>) -> &[Key] {
        match parent {
            Some(key) => &self.slot(key).children,
            None => &self.pages,
        }
    }

    /// Instances whose `props.componentId` is `master`.
    pub(crate) fn users(&self, master: &str) -> impl Iterator<Item = Key> + '_ {
        self.users.get(master).into_iter().flatten().copied()
    }

    pub(crate) fn has_users(&self, master: &str) -> bool {
        self.users.contains_key(master)
    }

    /// Nodes with a `props.links` entry whose `to` is `target`.
    pub(crate) fn linking(&self, target: &str) -> impl Iterator<Item = Key> + '_ {
        self.links.get(target).into_iter().flatten().copied()
    }

    /// Live slots in arena order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (Key, &Slot)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| slot.as_ref().map(|s| (Key(i as u32), s)))
    }

    /// Strict ancestors of `key`, nearest first.
    pub(crate) fn ancestors(&self, key: Key) -> impl Iterator<Item = Key> + '_ {
        std::iter::successors(self.slot(key).parent, |k| self.slot(*k).parent)
    }

    /// `root` and its descendants in preorder.
    pub(crate) fn subtree_keys(&self, root: Key) -> Vec<Key> {
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(key) = stack.pop() {
            out.push(key);
            stack.extend(self.slot(key).children.iter().rev());
        }
        out
    }
}

/// Estimated heap bytes of one node beyond its arena slot: id (in the node and as the
/// id-map key), parent id, id-map entry, its entry in a sibling list and props.
pub(crate) fn node_bytes(node: &Node) -> usize {
    2 * string_bytes(&node.id)
        + node.parent_id.as_deref().map_or(0, string_bytes)
        // Id-map slot at about 7/8 load, plus amortized sibling-list growth.
        + std::mem::size_of::<(String, Key)>() + 8
        + 2 * std::mem::size_of::<Key>()
        + props::heap_bytes(&node.props)
}

pub(crate) fn string_bytes(s: &str) -> usize {
    s.len() + props::ALLOC
}
