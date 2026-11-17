//! Reference indexes derived from one node's own kind and props: instance usage by
//! `componentId` and incoming links by `links[].to`. Every mutation that adds, removes
//! or replaces props (and its undo) calls these, so they are never logged themselves.
use super::{Key, Store};
use crate::props::{component_id, link_targets};
use fagent_scene_input::{Node, NodeKind};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};

/// The `componentId` an instance counts as a user of.
fn used_master<'a>(kind: &NodeKind, props: &'a Value) -> Option<&'a str> {
    match kind {
        NodeKind::Instance => component_id(props),
        _ => None,
    }
}

fn add(map: &mut HashMap<String, BTreeSet<Key>>, id: &str, key: Key) {
    match map.get_mut(id) {
        Some(set) => {
            set.insert(key);
        }
        None => {
            map.insert(id.to_owned(), BTreeSet::from([key]));
        }
    }
}

fn remove(map: &mut HashMap<String, BTreeSet<Key>>, id: &str, key: Key) {
    if let Some(set) = map.get_mut(id) {
        set.remove(&key);
        if set.is_empty() {
            map.remove(id);
        }
    }
}

impl Store {
    pub(super) fn index_refs(&mut self, key: Key, node: &Node) {
        if let Some(master) = used_master(&node.kind, &node.props) {
            add(&mut self.users, master, key);
        }
        for target in link_targets(&node.props) {
            add(&mut self.links, target, key);
        }
    }

    pub(super) fn unindex_refs(&mut self, key: Key, node: &Node) {
        if let Some(master) = used_master(&node.kind, &node.props) {
            remove(&mut self.users, master, key);
        }
        for target in link_targets(&node.props) {
            remove(&mut self.links, target, key);
        }
    }

    /// Re-indexes `key` after its props changed from `old` to its current props.
    pub(super) fn reindex_refs(&mut self, key: Key, old: &Value) {
        let node = self.slots[key.index()].take().expect("live node key");
        let kind = &node.node.kind;
        if used_master(kind, old) != used_master(kind, &node.node.props) {
            if let Some(master) = used_master(kind, old) {
                remove(&mut self.users, master, key);
            }
            if let Some(master) = used_master(kind, &node.node.props) {
                add(&mut self.users, master, key);
            }
        }
        if old.get("links").is_none() && node.node.props.get("links").is_none() {
            self.slots[key.index()] = Some(node);
            return;
        }
        let before: BTreeSet<&str> = link_targets(old).collect();
        let after: BTreeSet<&str> = link_targets(&node.node.props).collect();
        for gone in before.difference(&after) {
            remove(&mut self.links, gone, key);
        }
        for new in after.difference(&before) {
            add(&mut self.links, new, key);
        }
        self.slots[key.index()] = Some(node);
    }
}

impl Store {
    /// Every usage entry: master id -> instances (diagnostics and tests).
    pub(crate) fn usage_index(&self) -> impl Iterator<Item = (&String, &BTreeSet<Key>)> {
        self.users.iter()
    }

    /// Every link entry: target id -> linking nodes (diagnostics and tests).
    pub(crate) fn link_index(&self) -> impl Iterator<Item = (&String, &BTreeSet<Key>)> {
        self.links.iter()
    }
}
