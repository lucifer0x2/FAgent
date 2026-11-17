//! Delta publication. Only changed payloads and affected index shards are copied.
use super::{
    view::{resource_bytes, ReadView},
    view_indexes::{links, member, page, references},
};
use crate::{Node, Note, Snapshot};
use serde_json::Value;
use std::{collections::BTreeSet, sync::Arc};

impl ReadView {
    /// Advance a committed view; `None` rows mean deletion. A resource snapshot is
    /// supplied only when tokens/styles changed (assets/fonts evict the HOT entry).
    pub fn advance<'a>(
        &self,
        revision: u64,
        nodes: impl IntoIterator<Item = (&'a str, Option<&'a Node>)>,
        notes: impl IntoIterator<Item = (&'a str, Option<&'a Note>)>,
        resources: Option<(Snapshot, &'a [String], &'a [String])>,
    ) -> Self {
        let mut next = self.clone();
        next.validation = Default::default();
        let mut page_roots = BTreeSet::new();
        for (id, after) in nodes {
            let before = self.node(id);
            let placement = match (before, after) {
                (Some(a), Some(b)) => a.parent_id != b.parent_id || a.kind != b.kind,
                _ => true,
            };
            if placement || before.map(|n| n.sort_key) != after.map(|n| n.sort_key) {
                Arc::make_mut(&mut next.index).placement(before, after);
            }
            if placement {
                page_roots.insert(id.to_owned());
            }
            if refs_changed(before, after) {
                let old = self
                    .index
                    .dependencies
                    .get(&format!("node:{id}"))
                    .map(|v| v.as_ref().clone())
                    .unwrap_or_default();
                let new = after.map(|n| references(&n.props)).unwrap_or_default();
                if old != new {
                    Arc::make_mut(&mut next.index).refs(id, &old, &new);
                }
            }
            let old_master = before.and_then(|n| n.props["componentId"].as_str());
            let new_master = after.and_then(|n| n.props["componentId"].as_str());
            if old_master != new_master {
                let index = Arc::make_mut(&mut next.index);
                match new_master {
                    Some(master) => index.masters.insert(id.into(), master.into()),
                    None => index.masters.remove(id),
                }
            }
            let old_user = before
                .filter(|n| n.kind == crate::NodeKind::Instance)
                .and(old_master);
            let new_user = after
                .filter(|n| n.kind == crate::NodeKind::Instance)
                .and(new_master);
            if old_user != new_user {
                let index = Arc::make_mut(&mut next.index);
                if let Some(master) = old_user {
                    member(&mut index.users, master, id, false);
                }
                if let Some(master) = new_user {
                    member(&mut index.users, master, id, true);
                }
            }
            if before.and_then(|n| n.props.get("links")) != after.and_then(|n| n.props.get("links"))
            {
                let old = before.map(links).unwrap_or_default();
                let new = after.map(links).unwrap_or_default();
                if old != new {
                    Arc::make_mut(&mut next.index).links(id, &old, &new);
                }
            }
            match after {
                Some(node) => next.nodes.insert(id.into(), Arc::new(node.clone())),
                None => next.nodes.remove(id),
            }
        }
        // A moved container changes the pages of descendants with no row change.
        let mut pending: Vec<_> = page_roots.into_iter().collect();
        let mut seen = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Some(children) = next.index.children.get(&id) {
                pending.extend(children.iter().map(|(_, id)| id.clone()));
            }
            let new_page = page(&next, &id);
            if next.index.pages.get(&id).map(String::as_str) != new_page.as_deref() {
                let index = Arc::make_mut(&mut next.index);
                match new_page {
                    Some(page) => index.pages.insert(id, page),
                    None => index.pages.remove(&id),
                }
            }
        }
        for (id, after) in notes {
            let before = self.notes.get(id).map(Arc::as_ref);
            if before.map(|n| &n.node_id) != after.map(|n| &n.node_id) {
                let index = Arc::make_mut(&mut next.index);
                if let Some(note) = before {
                    member(&mut index.anchors, &note.node_id, id, false);
                }
                if let Some(note) = after {
                    member(&mut index.anchors, &note.node_id, id, true);
                }
            }
            match after {
                Some(note) => next.notes.insert(id.into(), Arc::new(note.clone())),
                None => next.notes.remove(id),
            }
        }
        let mut meta = if let Some((meta, tokens, styles)) = resources {
            for token in tokens
                .iter()
                .filter_map(|id| meta.tokens.iter().find(|t| t.id == *id))
            {
                next.resource_refs(format!("token:{}", token.id), references(&token.value));
            }
            for style in styles
                .iter()
                .filter_map(|id| meta.styles.iter().find(|s| s.id == *id))
            {
                next.resource_refs(format!("style:{}", style.id), references(&style.value));
            }
            next.resource_bytes = resource_bytes(&meta);
            meta
        } else {
            self.snapshot.as_ref().clone()
        };
        meta.revision = revision;
        next.snapshot = Arc::new(meta);
        next
    }

    fn resource_refs(&mut self, key: String, refs: BTreeSet<String>) {
        if self
            .index
            .dependencies
            .get(&key)
            .is_none_or(|old| **old != refs)
        {
            Arc::make_mut(&mut self.index)
                .dependencies
                .insert(key, Arc::new(refs));
        }
    }
}

fn refs_changed(before: Option<&Node>, after: Option<&Node>) -> bool {
    let (Some(before), Some(after)) = (before, after) else {
        return true;
    };
    let Some(old) = before.props.as_object() else {
        return true;
    };
    let Some(new) = after.props.as_object() else {
        return true;
    };
    old.keys().chain(new.keys()).any(|key| {
        old.get(key) != new.get(key) && (has_ref(key, old.get(key)) || has_ref(key, new.get(key)))
    })
}

fn has_ref(key: &str, value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(_)) => matches!(
            key,
            "tokenId" | "aliasId" | "styleId" | "fontId" | "assetId" | "componentId" | "to"
        ),
        Some(value @ (Value::Object(_) | Value::Array(_))) => !references(value).is_empty(),
        _ => false,
    }
}
