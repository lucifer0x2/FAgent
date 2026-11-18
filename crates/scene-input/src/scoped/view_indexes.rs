//! Persistent reader-format topology, inverse references, pages and dependencies.
use super::{persistent::Map, view::ReadView};
use crate::{Node, NodeKind};
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

pub(super) type Ids = Arc<BTreeSet<String>>;
pub(super) type Siblings = Arc<BTreeSet<(i64, String)>>;
/// Sets under construction, keyed like the index they are stored in.
type Sets = HashMap<String, BTreeSet<String>>;

#[derive(Clone, Default)]
pub(super) struct ViewIndexes {
    pub children: Map<Siblings>,
    pub roots: Siblings,
    pub frames: Arc<BTreeSet<(Option<String>, i64, String)>>,
    pub anchors: Map<Ids>,
    pub incoming: Map<Ids>,
    pub pages: Map<String>,
    /// Resource/component/link key -> authored node users (including overrides).
    pub references: Map<Ids>,
    /// Node/resource key -> referenced keys. Component edges drive expansion.
    pub dependencies: Map<Ids>,
    /// Expansion follows only top-level componentId, unlike generic inverse refs.
    pub masters: Map<String>,
    pub users: Map<Ids>,
}

impl ViewIndexes {
    pub fn new(view: &ReadView) -> Self {
        let mut index = Self::default();
        // Sets are built whole, then stored once: adding members one at a time to a
        // stored set copies and weighs it per member, quadratic for wide parents.
        let mut children: HashMap<String, BTreeSet<(i64, String)>> = HashMap::new();
        let (mut refs, mut incoming, mut users) = (Sets::new(), Sets::new(), Sets::new());
        for (_, node) in view.nodes.iter() {
            let placed = (node.sort_key, node.id.clone());
            match &node.parent_id {
                Some(parent) => {
                    children.entry(parent.clone()).or_default().insert(placed);
                }
                None => {
                    Arc::make_mut(&mut index.roots).insert(placed);
                }
            }
            if node.kind == NodeKind::Frame {
                let frame = (node.parent_id.clone(), node.sort_key, node.id.clone());
                Arc::make_mut(&mut index.frames).insert(frame);
            }
            let keys = references(&node.props);
            for key in &keys {
                refs.entry(key.clone()).or_default().insert(node.id.clone());
            }
            if !keys.is_empty() {
                index
                    .dependencies
                    .insert(format!("node:{}", node.id), Arc::new(keys));
            }
            for key in links(node) {
                incoming.entry(key).or_default().insert(node.id.clone());
            }
            if let Some(master) = node.typed_props().component_id() {
                index.masters.insert(node.id.clone(), master.into());
                if node.kind == NodeKind::Instance {
                    users
                        .entry(master.into())
                        .or_default()
                        .insert(node.id.clone());
                }
            }
        }
        for (parent, ids) in children {
            index.children.insert(parent, Arc::new(ids));
        }
        for (sets, map) in [
            (refs, &mut index.references),
            (incoming, &mut index.incoming),
            (users, &mut index.users),
        ] {
            for (key, ids) in sets {
                map.insert(key, Arc::new(ids));
            }
        }
        for (id, _) in view.nodes.iter() {
            if let Some(page) = page(view, id) {
                index.pages.insert(id.clone(), page);
            }
        }
        let mut anchors = Sets::new();
        for (_, note) in view.notes.iter() {
            let notes = anchors.entry(note.node_id.clone()).or_default();
            notes.insert(note.id.clone());
        }
        for (node, ids) in anchors {
            index.anchors.insert(node, Arc::new(ids));
        }
        for token in &view.snapshot.tokens {
            index.dependencies.insert(
                format!("token:{}", token.id),
                Arc::new(references(&token.value)),
            );
        }
        for style in &view.snapshot.styles {
            index.dependencies.insert(
                format!("style:{}", style.id),
                Arc::new(references(&style.value)),
            );
        }
        for font in &view.snapshot.fonts {
            let mut keys = BTreeSet::from([format!("font-family:{}", font.family)]);
            if let Some(asset) = &font.asset_id {
                keys.insert(format!("asset:{asset}"));
            }
            index
                .dependencies
                .insert(format!("font:{}", font.id), Arc::new(keys));
        }
        index
    }

    pub fn placement(&mut self, before: Option<&Node>, after: Option<&Node>) {
        if let Some(node) = before {
            match &node.parent_id {
                Some(parent) => sibling(&mut self.children, parent, node, false),
                None => {
                    Arc::make_mut(&mut self.roots).remove(&(node.sort_key, node.id.clone()));
                }
            }
            if node.kind == NodeKind::Frame {
                Arc::make_mut(&mut self.frames).remove(&(
                    node.parent_id.clone(),
                    node.sort_key,
                    node.id.clone(),
                ));
            }
        }
        if let Some(node) = after {
            match &node.parent_id {
                Some(parent) => sibling(&mut self.children, parent, node, true),
                None => {
                    Arc::make_mut(&mut self.roots).insert((node.sort_key, node.id.clone()));
                }
            }
            if node.kind == NodeKind::Frame {
                Arc::make_mut(&mut self.frames).insert((
                    node.parent_id.clone(),
                    node.sort_key,
                    node.id.clone(),
                ));
            }
        }
    }

    pub fn refs(&mut self, id: &str, before: &BTreeSet<String>, after: &BTreeSet<String>) {
        if before == after {
            return;
        }
        for key in before.difference(after) {
            member(&mut self.references, key, id, false);
        }
        for key in after.difference(before) {
            member(&mut self.references, key, id, true);
        }
        if after.is_empty() {
            self.dependencies.remove(&format!("node:{id}"));
        } else {
            self.dependencies
                .insert(format!("node:{id}"), Arc::new(after.clone()));
        }
    }

    pub fn links(&mut self, id: &str, before: &BTreeSet<String>, after: &BTreeSet<String>) {
        for key in before.difference(after) {
            member(&mut self.incoming, key, id, false);
        }
        for key in after.difference(before) {
            member(&mut self.incoming, key, id, true);
        }
    }

    pub fn estimated_bytes(&self) -> usize {
        let maps = [
            &self.anchors,
            &self.incoming,
            &self.references,
            &self.dependencies,
            &self.users,
        ];
        maps.into_iter().map(Map::estimated_bytes).sum::<usize>()
            + self.children.estimated_bytes()
            + self.pages.estimated_bytes()
            + self.roots.len() * 256
            // Frames also retain the parent ID in their source-order key.
            + self.frames.len() * 512
            + self.masters.estimated_bytes()
    }
}

fn sibling(map: &mut Map<Siblings>, parent: &str, node: &Node, add: bool) {
    let mut ids = map.get(parent).cloned().unwrap_or_default();
    let key = (node.sort_key, node.id.clone());
    if add {
        Arc::make_mut(&mut ids).insert(key);
    } else {
        Arc::make_mut(&mut ids).remove(&key);
    }
    if ids.is_empty() {
        map.remove(parent);
    } else {
        map.insert(parent.into(), ids);
    }
}

pub(super) fn links(node: &Node) -> BTreeSet<String> {
    crate::node_links(&node.props)
        .into_iter()
        .filter_map(|link| link.to)
        .collect()
}

pub(super) fn member(map: &mut Map<Ids>, key: &str, id: &str, add: bool) {
    let mut ids = map.get(key).cloned().unwrap_or_default();
    if add {
        Arc::make_mut(&mut ids).insert(id.into());
    } else {
        Arc::make_mut(&mut ids).remove(id);
    }
    if ids.is_empty() {
        map.remove(key);
    } else {
        map.insert(key.into(), ids);
    }
}

pub(super) fn page(view: &ReadView, id: &str) -> Option<String> {
    let mut current = view.node(id);
    for _ in 0..=128 {
        let node = current?;
        if node.kind == NodeKind::Page {
            return Some(node.id.clone());
        }
        current = node.parent_id.as_deref().and_then(|id| view.node(id));
    }
    None
}

/// Scan only a touched row (including nested overrides/state paints).
pub(super) fn references(value: &Value) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    scan(value, &mut keys);
    keys
}

fn scan(value: &Value, keys: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let kind = match key.as_str() {
                    "tokenId" | "aliasId" => Some("token"),
                    "styleId" => Some("style"),
                    "fontId" => Some("font"),
                    "assetId" => Some("asset"),
                    "componentId" => Some("component"),
                    "to" => Some("link"),
                    _ => None,
                };
                if let (Some(kind), Some(id)) = (kind, value.as_str()) {
                    keys.insert(format!("{kind}:{id}"));
                }
                scan(value, keys);
            }
        }
        Value::Array(values) => {
            for value in values {
                scan(value, keys);
            }
        }
        _ => {}
    }
}
