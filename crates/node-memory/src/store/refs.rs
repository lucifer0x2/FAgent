//! Reference indexes derived from one node's own kind and props: instance usage by
//! `componentId`, incoming links by `links[].to`, and every key `reference_keys`
//! names (`token:`, `style:`, `font:`, `asset:`, `component:`, `link:`, nested
//! overrides included, plus `theme:` for the node's own `theme`). Every mutation that
//! adds, removes or replaces props calls these.
use super::trie::Trie;
use super::{Key, Set, Store};
use crate::props::{component_id, link_targets};
use fagent_scene_input::validate::reference_keys;
use fagent_scene_input::{Node, NodeKind};
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::Arc;

/// Bytes counted per index membership.
const MEMBER: usize = 16;

/// The `componentId` an instance counts as a user of.
fn used_master<'a>(kind: &NodeKind, props: &'a Value) -> Option<&'a str> {
    match kind {
        NodeKind::Instance => component_id(props),
        _ => None,
    }
}

fn member(index: &mut Trie<Set>, at: u32, key: Key, add: bool) -> bool {
    let changed = match (index.get_mut(at), add) {
        (Some(set), true) => Arc::make_mut(set).insert(key),
        (Some(set), false) => Arc::make_mut(set).remove(&key),
        (None, true) => {
            index.insert(at, Arc::new(BTreeSet::from([key])));
            return true;
        }
        (None, false) => false,
    };
    if !add && index.get(at).is_some_and(|set| set.is_empty()) {
        index.remove(at);
    }
    changed
}

impl Store {
    fn link_member(&mut self, target: &str, key: Key, add: bool) {
        let at = self.ids.intern(target);
        if member(&mut self.links, at, key, add) {
            self.account(add);
        }
    }

    fn user_member(&mut self, master: &str, key: Key, add: bool) {
        let at = self.ids.intern(master);
        if member(&mut self.users, at, key, add) {
            self.account(add);
        }
    }

    fn ref_member(&mut self, name: &str, key: Key, add: bool) {
        let at = self.ref_keys.intern(name);
        if member(&mut self.refs, at, key, add) {
            self.account(add);
        }
    }

    fn account(&mut self, add: bool) {
        match add {
            true => self.bytes += MEMBER,
            false => self.bytes -= MEMBER,
        }
    }

    pub(super) fn index_refs(&mut self, key: Key, node: &Node) {
        if let Some(master) = used_master(&node.kind, &node.props) {
            self.user_member(master, key, true);
        }
        for target in link_targets(&node.props) {
            self.link_member(target, key, true);
        }
        for name in reference_keys(&node.props) {
            self.ref_member(&name, key, true);
        }
    }

    pub(super) fn unindex_refs(&mut self, key: Key, node: &Node) {
        if let Some(master) = used_master(&node.kind, &node.props) {
            self.user_member(master, key, false);
        }
        for target in link_targets(&node.props) {
            self.link_member(target, key, false);
        }
        for name in reference_keys(&node.props) {
            self.ref_member(&name, key, false);
        }
    }

    /// Re-indexes `key` after its props changed from `old` to its current props.
    pub(super) fn reindex_refs(&mut self, key: Key, old: &Value) {
        let node = self.slot(key).node.clone();
        let kind = &node.kind;
        if used_master(kind, old) != used_master(kind, &node.props) {
            if let Some(master) = used_master(kind, old) {
                self.user_member(master, key, false);
            }
            if let Some(master) = used_master(kind, &node.props) {
                self.user_member(master, key, true);
            }
        }
        if old.get("links").is_some() || node.props.get("links").is_some() {
            let before: BTreeSet<&str> = link_targets(old).collect();
            let after: BTreeSet<&str> = link_targets(&node.props).collect();
            for gone in before.difference(&after) {
                self.link_member(gone, key, false);
            }
            for new in after.difference(&before) {
                self.link_member(new, key, true);
            }
        }
        let (before, after) = (reference_keys(old), reference_keys(&node.props));
        for gone in before.difference(&after) {
            self.ref_member(gone, key, false);
        }
        for new in after.difference(&before) {
            self.ref_member(new, key, true);
        }
    }
}

impl Store {
    /// Every usage entry: master id -> instances (diagnostics and tests).
    pub(crate) fn usage_index(&self) -> Vec<(String, Vec<Key>)> {
        index_entries(&self.users, |k| self.ids.name(k).to_string())
    }

    /// Every link entry: target id -> linking nodes (diagnostics and tests).
    pub(crate) fn link_index(&self) -> Vec<(String, Vec<Key>)> {
        index_entries(&self.links, |k| self.ids.name(k).to_string())
    }

    /// Every reference entry: key -> referencing nodes (diagnostics and tests).
    pub(crate) fn reference_index(&self) -> Vec<(String, Vec<Key>)> {
        index_entries(&self.refs, |k| self.ref_keys.name(k).to_string())
    }
}

fn index_entries(index: &Trie<Set>, name: impl Fn(u32) -> String) -> Vec<(String, Vec<Key>)> {
    let entries = index
        .iter()
        .map(|(k, set)| (name(k), set.iter().copied().collect()));
    entries.collect()
}
