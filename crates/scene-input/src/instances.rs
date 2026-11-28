//! One expansion boundary shared by compute and reads. Persistence stays authored.
use crate::{Node, NodeKind, Result, StorageError, MAX_NODES};
use serde_json::Value;
use std::collections::HashMap;

pub(crate) const MAX_TREE_DEPTH: usize = 128;

/// Stored rows are trusted on scoped reads; malformed props must fail, never panic.
fn not_object() -> StorageError {
    StorageError::validation("props", "Properties must be an object")
}

/// Keys an `overrides.<descendantId>` patch may set on a master descendant. Font keys
/// (`fontSize`, `lineHeight`, `fontId`, `fontWeight`, `fontStyle`) and the position (`x`,
/// `y`) stay master-only. A `<nestedInstanceId>/<descendantId>` target reaches through a
/// nested instance (see [`expand`]).
pub const OVERRIDE_KEYS: &[&str] = &[
    "text",
    "fill",
    "visible",
    "styleId",
    "assetId",
    "name",
    "stroke",
    "shadow",
    "states",
    "opacity",
    "radius",
    "clip",
    "textDecoration",
    "textTransform",
    "textAlign",
    "letterSpacing",
    "textWrap",
    "width",
    "height",
    "minWidth",
    "maxWidth",
    "minHeight",
    "maxHeight",
    "layout",
];
/// Keys an instance node may author; all but `componentId` and `overrides` patch the
/// expanded root (`stroke`, `layout` and `states` merge, the rest replace).
pub const INSTANCE_KEYS: &[&str] = &[
    "componentId",
    "overrides",
    "name",
    "x",
    "y",
    "width",
    "height",
    "minWidth",
    "maxWidth",
    "minHeight",
    "maxHeight",
    "visible",
    "theme",
    "fill",
    "stroke",
    "shadow",
    "radius",
    "opacity",
    "states",
    "clip",
    "layout",
];

/// Hands a nested instance the outer overrides addressed through it: a key
/// `<child>/<rest>` becomes `<rest>` among the nested instance's own overrides, and the
/// outer patch wins key by key. Deeper paths keep their tail for the next level.
fn pass_through(nested: &mut Node, child: &str, outer: &Value) -> Result<()> {
    let prefix = format!("{child}/");
    let through: serde_json::Map<String, Value> = outer
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(key, patch)| Some((key.strip_prefix(&prefix)?.to_owned(), patch.clone())))
        .collect();
    if through.is_empty() {
        return Ok(());
    }
    let props = nested.props.as_object_mut().ok_or_else(not_object)?;
    let mut own = props
        .get("overrides")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    crate::instance_paint::merge_overrides(&mut own, through);
    props.insert("overrides".into(), Value::Object(own));
    Ok(())
}

/// Expands references before resolving styles/tokens, preserving stable virtual IDs.
/// Virtual IDs join the instance path with `/` (`card/btn/label`); an override key uses the
/// same path below the instance (`btn/label`) to reach into nested instances.
pub fn expand(nodes: &[Node]) -> Result<Vec<Node>> {
    if !nodes.iter().any(|n| n.kind == NodeKind::Instance) {
        return Ok(nodes.to_vec());
    }
    let nodes: Vec<_> = nodes.iter().collect();
    expand_selected(&nodes, &nodes)
}

/// Authored lookups for expansion: a bounded closure or a revision-bound index.
/// Children must be returned in snapshot source order.
pub(crate) trait Lookup {
    fn node(&self, id: &str) -> Option<&Node>;
    fn children(&self, id: &str) -> Children<'_>;
}

/// Allocation-free child iteration over either lookup representation.
pub(crate) enum Children<'s> {
    Nodes(std::slice::Iter<'s, &'s Node>),
    Positions(std::slice::Iter<'s, usize>, &'s [Node]),
    Indexed(Box<dyn Iterator<Item = &'s Node> + 's>),
}

impl<'s> Iterator for Children<'s> {
    type Item = &'s Node;
    fn next(&mut self) -> Option<&'s Node> {
        match self {
            Self::Nodes(nodes) => nodes.next().copied(),
            Self::Positions(positions, nodes) => positions.next().map(|&i| &nodes[i]),
            Self::Indexed(nodes) => nodes.next(),
        }
    }
}

struct Closure<'a> {
    by_id: HashMap<&'a str, &'a Node>,
    children: HashMap<&'a str, Vec<&'a Node>>,
}

impl Lookup for Closure<'_> {
    fn node(&self, id: &str) -> Option<&Node> {
        self.by_id.get(id).copied()
    }
    fn children(&self, id: &str) -> Children<'_> {
        Children::Nodes(self.children.get(id).map_or([].iter(), |c| c.iter()))
    }
}

/// Expand selected authored nodes using an already bounded dependency closure.
/// Both slices retain source order; masters are lookup-only unless selected.
fn expand_selected(nodes: &[&Node], selected: &[&Node]) -> Result<Vec<Node>> {
    let by_id = nodes.iter().map(|&n| (n.id.as_str(), n)).collect();
    let mut children: HashMap<&str, Vec<&Node>> = HashMap::new();
    for &node in nodes {
        if let Some(parent) = node.parent_id.as_deref() {
            children.entry(parent).or_default().push(node);
        }
    }
    expand_with(&Closure { by_id, children }, selected, MAX_NODES)
}

/// Expand `selected` in order, failing past `limit` nodes. Any lookup that resolves
/// every reachable master and its complete subtree produces the same output as the
/// full expansion.
pub(crate) fn expand_with(
    lookup: &impl Lookup,
    selected: &[&Node],
    limit: usize,
) -> Result<Vec<Node>> {
    let mut out = Vec::new();
    let expander = Expander { lookup, limit };
    for &node in selected {
        expander.node(node.clone(), 0, &mut out)?;
    }
    Ok(out)
}

struct Expander<'l, L> {
    lookup: &'l L,
    limit: usize,
}
impl<L: Lookup> Expander<'_, L> {
    fn node(&self, node: Node, depth: usize, out: &mut Vec<Node>) -> Result<()> {
        if out.len() >= self.limit {
            return Err(StorageError::new(
                "INSTANCE_LIMIT",
                &format!("Expanded project exceeds {} nodes", self.limit),
            ));
        }
        if node.kind != NodeKind::Instance {
            out.push(node);
            return Ok(());
        }
        if depth >= 8 {
            return Err(StorageError::new(
                "INSTANCE_DEPTH",
                "Component nesting exceeds 8",
            ));
        }
        let master = node.props["componentId"]
            .as_str()
            .and_then(|id| self.lookup.node(id))
            .filter(|n| n.kind == NodeKind::Frame && n.props.get("component").is_some())
            .ok_or_else(|| {
                StorageError::validation("props.componentId", "Expected a component frame")
            })?;
        let mut root = node.clone();
        root.kind = NodeKind::Frame;
        root.props = crate::instance_paint::instance_root_props(&master.props, &node.props)
            .ok_or_else(not_object)?;
        out.push(root);
        self.descendants(
            &master.id,
            &node.id,
            &node.id,
            &node.props["overrides"],
            (depth + 1, 1),
            out,
        )
    }

    fn descendants(
        &self,
        source: &str,
        parent: &str,
        prefix: &str,
        overrides: &Value,
        (depth, level): (usize, usize),
        out: &mut Vec<Node>,
    ) -> Result<()> {
        // A validated master subtree is at most 128 levels deep; deeper walks
        // can only come from a cycle or corrupt stored rows.
        if level > MAX_TREE_DEPTH {
            return Err(StorageError::validation(
                "parentId",
                "Tree exceeds maximum depth 128",
            ));
        }
        for child in self.lookup.children(source) {
            let mut virtual_node = child.clone();
            virtual_node.id = format!("{prefix}/{}", child.id);
            virtual_node.parent_id = Some(parent.into());
            if let Some(patch) = overrides.get(&child.id).and_then(Value::as_object) {
                virtual_node.props.as_object().ok_or_else(not_object)?;
                crate::instance_paint::merge_props(&mut virtual_node.props, patch);
            }
            let id = virtual_node.id.clone();
            let nested = virtual_node.kind == NodeKind::Instance;
            if nested {
                pass_through(&mut virtual_node, &child.id, overrides)?;
            }
            self.node(virtual_node, depth, out)?;
            if !nested {
                self.descendants(&child.id, &id, prefix, overrides, (depth, level + 1), out)?;
            }
        }
        Ok(())
    }
}
