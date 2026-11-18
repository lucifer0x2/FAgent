//! One persistent project state: nodes and every structural index in radix tries
//! keyed by interned ids. Cloning a [`Store`] is O(1) (a handful of `Arc` bumps); a
//! batch writes on its clone, copying only the trie paths and nodes it touches, and
//! is either kept whole or dropped whole, so no undo log exists. Reads live here;
//! mutations in `mutate`, sibling order in `order`, the props-derived reference
//! indexes in `refs`, resources in `resources` and per-property write revisions in
//! `stamps`.
mod derive;
mod intern;
mod lookup;
mod mutate;
mod order;
mod refs;
mod resources;
mod stamps;
pub(crate) mod trie;

use crate::props;
use fagent_scene_input::Node;
use intern::Interner;
pub(crate) use resources::Resources;
pub(crate) use stamps::{Prop, Stamps};
use std::collections::BTreeSet;
use std::sync::Arc;
use trie::Trie;

/// Compact node handle: the interned id. Never reused for another id.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub(crate) struct Key(u32);

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

/// One live node with its links and derived indexes. `node` and `children` are shared
/// with every state version that did not rewrite them.
#[derive(Clone, Debug)]
pub(crate) struct Slot {
    pub node: Arc<Node>,
    pub parent: Option<Key>,
    /// Children ordered by `(sort_key, id)`, the stored sibling order.
    pub children: Arc<Vec<Key>>,
    pub derived: Derived,
    /// Revisions of writes since load, per property; empty until the first write.
    pub stamps: Stamps,
}

pub(crate) type Set = Arc<BTreeSet<Key>>;

#[derive(Clone, Default)]
pub(crate) struct Store {
    /// Node ids, and ids named by `componentId` or `links[].to` (live or not).
    ids: Arc<Interner>,
    /// Reference keys (`token:<id>`, `style:<id>`, ...) for `refs`.
    ref_keys: Arc<Interner>,
    /// Top-level props keys for stamps.
    props: Arc<stamps::PropNames>,
    slots: Trie<Slot>,
    /// Pages (children of no parent), ordered like any sibling list.
    pages: Arc<Vec<Key>>,
    /// Instances by the interned `props.componentId` (live master or not).
    users: Trie<Set>,
    /// Nodes by the interned ids their `props.links[].to` name.
    links: Trie<Set>,
    /// Nodes by the interned reference keys their props name (nested included).
    refs: Trie<Set>,
    pub(crate) res: Resources,
    /// Estimated bytes of nodes, stamps and index entries (not trie nodes or names).
    bytes: usize,
}

impl Store {
    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }

    /// Estimated bytes of this version, counting shared parts in full.
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
            + self.slots.node_bytes()
            + self.users.node_bytes()
            + self.links.node_bytes()
            + self.refs.node_bytes()
            + self.ids.bytes()
            + self.ref_keys.bytes()
    }

    /// Estimated node bytes without trie nodes and interned names.
    pub(crate) fn content_bytes(&self) -> usize {
        self.bytes
    }

    pub(crate) fn key(&self, id: &str) -> Option<Key> {
        let key = Key(self.ids.get(id)?);
        self.slots.contains(key.0).then_some(key)
    }

    pub(crate) fn contains(&self, id: &str) -> bool {
        self.key(id).is_some()
    }

    /// The slot of a live key. Keys handed out by this store are live until removed,
    /// so a dead key is an internal invariant violation.
    pub(crate) fn slot(&self, key: Key) -> &Slot {
        self.slots.get(key.0).expect("live node key")
    }

    /// The slot of `key` when it is live.
    pub(crate) fn live(&self, key: Key) -> Option<&Slot> {
        self.slots.get(key.0)
    }

    pub(crate) fn get(&self, id: &str) -> Option<&Slot> {
        self.slots.get(self.ids.get(id)?)
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

    fn members<'a>(
        &'a self,
        index: &'a Trie<Set>,
        key: Option<u32>,
    ) -> impl Iterator<Item = Key> + 'a {
        key.and_then(|k| index.get(k))
            .into_iter()
            .flat_map(|s| s.iter().copied())
    }

    /// Instances whose `props.componentId` is `master`.
    pub(crate) fn users(&self, master: &str) -> impl Iterator<Item = Key> + '_ {
        self.members(&self.users, self.ids.get(master))
    }

    pub(crate) fn has_users(&self, master: &str) -> bool {
        self.users(master).next().is_some()
    }

    /// Nodes with a `props.links` entry whose `to` is `target`.
    pub(crate) fn linking(&self, target: &str) -> impl Iterator<Item = Key> + '_ {
        self.members(&self.links, self.ids.get(target))
    }

    /// Nodes whose props reference `key` (`token:<id>`, `component:<id>`, ...).
    pub(crate) fn referencing(&self, key: &str) -> impl Iterator<Item = Key> + '_ {
        self.members(&self.refs, self.ref_keys.get(key))
    }

    /// Live slots in key order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (Key, &Slot)> {
        self.slots.iter().map(|(k, slot)| (Key(k), slot))
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

    /// Slots whose entry differs between `self` and `base` (an ancestor version),
    /// found by pointer: shared trie nodes are skipped, and a slot counts as changed
    /// when its node, links or derived values were rewritten.
    pub(crate) fn changed_since(&self, base: &Store) -> Vec<Key> {
        let same = |a: &Slot, b: &Slot| {
            Arc::ptr_eq(&a.node, &b.node)
                && Arc::ptr_eq(&a.children, &b.children)
                && (a.parent, a.derived) == (b.parent, b.derived)
        };
        self.slots
            .diff(&base.slots, same)
            .into_iter()
            .map(Key)
            .collect()
    }
}

/// Estimated heap bytes of one node beyond its trie slot: the shared `Node`
/// allocation, id, parent id, its entry in a sibling list and props.
pub(crate) fn node_bytes(node: &Node) -> usize {
    std::mem::size_of::<Node>()
        + props::ALLOC
        + string_bytes(&node.id)
        + node.parent_id.as_deref().map_or(0, string_bytes)
        + std::mem::size_of::<Key>()
        + props::heap_bytes(&node.props)
}

pub(crate) fn string_bytes(s: &str) -> usize {
    s.len() + props::ALLOC
}

/// Bytes of one trie slot.
pub(crate) const SLOT_SIZE: usize = std::mem::size_of::<Option<Slot>>();
