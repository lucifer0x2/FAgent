//! [`ViewSource`] over a full snapshot, indexed once: topology, pages, inverse
//! references and note anchors. Used where no engine state exists (tests, bundles,
//! the oracle); it is immutable, so it has no incremental update path.
use super::view::ViewSource;
use crate::{Node, NodeKind, Note};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};

type Sets = HashMap<String, BTreeSet<String>>;

pub(super) struct SnapshotSource {
    nodes: HashMap<String, Node>,
    notes: BTreeMap<String, Note>,
    /// Children ids per parent in `(sort_key, id)` order; pages under `None`.
    children: HashMap<Option<String>, Vec<String>>,
    pages: HashMap<String, String>,
    anchors: Sets,
    incoming: Sets,
    users: Sets,
    references: Sets,
    bytes: usize,
}

impl SnapshotSource {
    pub(super) fn new(nodes: Vec<Node>, notes: Vec<Note>) -> Self {
        let mut placed: HashMap<Option<String>, Vec<(i64, String)>> = HashMap::new();
        let (mut incoming, mut users, mut references) = (Sets::new(), Sets::new(), Sets::new());
        let mut bytes = 0;
        for node in &nodes {
            bytes += node_bytes(node);
            let siblings = placed.entry(node.parent_id.clone()).or_default();
            siblings.push((node.sort_key, node.id.clone()));
            for key in references_of(&node.props) {
                references.entry(key).or_default().insert(node.id.clone());
            }
            for link in crate::node_links(&node.props) {
                if let Some(to) = link.to {
                    incoming.entry(to).or_default().insert(node.id.clone());
                }
            }
            if node.kind == NodeKind::Instance {
                if let Some(master) = node.typed_props().component_id() {
                    let set = users.entry(master.to_owned()).or_default();
                    set.insert(node.id.clone());
                }
            }
        }
        let children = placed
            .into_iter()
            .map(|(parent, mut ids)| {
                ids.sort();
                (parent, ids.into_iter().map(|(_, id)| id).collect())
            })
            .collect();
        let nodes: HashMap<String, Node> = nodes.into_iter().map(|n| (n.id.clone(), n)).collect();
        let pages = nodes
            .keys()
            .filter_map(|id| Some((id.clone(), page_of(&nodes, id)?)))
            .collect();
        let mut anchors = Sets::new();
        let notes: BTreeMap<String, Note> = notes
            .into_iter()
            .map(|note| {
                bytes += note_bytes(&note);
                let set = anchors.entry(note.node_id.clone()).or_default();
                set.insert(note.id.clone());
                (note.id.clone(), note)
            })
            .collect();
        Self {
            nodes,
            notes,
            children,
            pages,
            anchors,
            incoming,
            users,
            references,
            bytes,
        }
    }

    fn sorted<'a>(&'a self, sets: &'a Sets, key: &str) -> Vec<&'a str> {
        let ids = sets.get(key).into_iter().flatten();
        ids.map(String::as_str).collect()
    }
}

impl ViewSource for SnapshotSource {
    fn node_count(&self) -> usize {
        self.nodes.len()
    }

    fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }

    fn note(&self, id: &str) -> Option<&Note> {
        self.notes.get(id)
    }

    fn children(&self, id: &str) -> Box<dyn Iterator<Item = &Node> + '_> {
        let ids = self
            .children
            .get(&Some(id.to_owned()))
            .into_iter()
            .flatten();
        Box::new(ids.filter_map(|id| self.nodes.get(id)))
    }

    fn child_count(&self, id: &str) -> usize {
        self.children.get(&Some(id.to_owned())).map_or(0, Vec::len)
    }

    fn roots(&self) -> Box<dyn Iterator<Item = &Node> + '_> {
        let ids = self.children.get(&None).into_iter().flatten();
        Box::new(ids.filter_map(|id| self.nodes.get(id)))
    }

    fn nodes(&self) -> Box<dyn Iterator<Item = &Node> + '_> {
        Box::new(self.nodes.values())
    }

    fn notes(&self) -> Box<dyn Iterator<Item = &Note> + '_> {
        Box::new(self.notes.values())
    }

    fn page(&self, id: &str) -> Option<&str> {
        self.pages.get(id).map(String::as_str)
    }

    fn notes_on(&self, id: &str) -> Vec<&Note> {
        let ids = self.anchors.get(id).into_iter().flatten();
        ids.filter_map(|id| self.notes.get(id)).collect()
    }

    fn component_users(&self, master: &str) -> Vec<&str> {
        self.sorted(&self.users, master)
    }

    fn linking(&self, target: &str) -> Vec<&str> {
        self.sorted(&self.incoming, target)
    }

    fn reference_users(&self, key: &str) -> Vec<&str> {
        self.sorted(&self.references, key)
    }

    fn estimated_bytes(&self) -> usize {
        self.bytes
    }
}

fn page_of(nodes: &HashMap<String, Node>, id: &str) -> Option<String> {
    let mut current = nodes.get(id);
    for _ in 0..=128 {
        let node = current?;
        if node.kind == NodeKind::Page {
            return Some(node.id.clone());
        }
        current = node.parent_id.as_deref().and_then(|id| nodes.get(id));
    }
    None
}

fn node_bytes(node: &Node) -> usize {
    128 + node.id.len()
        + node.parent_id.as_ref().map_or(0, String::len)
        + node.props.to_string().len()
}

fn note_bytes(note: &Note) -> usize {
    128 + serde_json::to_vec(note).map_or(0, |v| v.len())
}

/// Reference keys a value names (nested overrides and state paints included):
/// `token:`, `style:`, `font:`, `asset:`, `component:` and `link:` + id.
pub fn references_of(value: &Value) -> BTreeSet<String> {
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
