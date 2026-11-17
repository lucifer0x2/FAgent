//! Id-keyed view of every index, independent of slot numbering, so tests can compare
//! an incrementally maintained project with a from-scratch rebuild.
use super::ProjectState;
use crate::store::Key;
use std::collections::{BTreeMap, BTreeSet};

/// Every index of a project keyed by node id. Children keep sibling order; pages are
/// listed under the empty parent id `""`.
#[doc(hidden)]
#[derive(Debug, Default, PartialEq, Eq)]
pub struct IndexDump {
    pub children: BTreeMap<String, Vec<String>>,
    pub parent: BTreeMap<String, Option<String>>,
    pub page_of: BTreeMap<String, String>,
    pub root_frame_of: BTreeMap<String, Option<String>>,
    pub master_of: BTreeMap<String, Option<String>>,
    /// Instances by `componentId`, sorted.
    pub instances_of: BTreeMap<String, Vec<String>>,
    /// Linking nodes by link target id, sorted.
    pub linked_from: BTreeMap<String, Vec<String>>,
    /// Note ids by anchor node id.
    pub notes_on: BTreeMap<String, Vec<String>>,
    pub len: usize,
    /// Estimated node bytes without write stamps, excluding the slot arena (which
    /// keeps freed slots).
    pub node_bytes: usize,
    /// Estimated bytes of write stamps (zero right after a load).
    pub stamp_bytes: usize,
}

impl ProjectState {
    /// Builds an [`IndexDump`]; O(project), for tests and diagnostics only.
    #[doc(hidden)]
    pub fn index_dump(&self) -> IndexDump {
        let store = &self.store;
        let id = |key: Key| store.node(key).id.clone();
        let ids = |keys: &[Key]| keys.iter().map(|k| id(*k)).collect::<Vec<_>>();
        let sorted = |keys: &BTreeSet<Key>| {
            let mut out: Vec<String> = keys.iter().map(|k| id(*k)).collect();
            out.sort();
            out
        };
        let mut dump = IndexDump {
            len: store.len(),
            ..IndexDump::default()
        };
        dump.children
            .insert(String::new(), ids(store.children(None)));
        for (key, slot) in store.iter() {
            let name = id(key);
            if !slot.children.is_empty() {
                dump.children.insert(name.clone(), ids(&slot.children));
            }
            let notes: Vec<String> = store.res.notes_on(&name).map(str::to_owned).collect();
            if !notes.is_empty() {
                dump.notes_on.insert(name.clone(), notes);
            }
            dump.stamp_bytes += slot.stamps.bytes();
            dump.parent.insert(name.clone(), slot.parent.map(id));
            dump.page_of.insert(name.clone(), id(slot.derived.page));
            dump.root_frame_of
                .insert(name.clone(), slot.derived.root.map(id));
            dump.master_of.insert(name, slot.derived.master.map(id));
        }
        dump.node_bytes = store.content_bytes() - dump.stamp_bytes;
        for (master, users) in store.usage_index() {
            dump.instances_of.insert(master.clone(), sorted(users));
        }
        for (target, sources) in store.link_index() {
            dump.linked_from.insert(target.clone(), sorted(sources));
        }
        dump
    }
}
