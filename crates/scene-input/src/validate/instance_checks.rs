//! Per-node instance checks over any [`Lookup`], shared by full validation and the delta
//! rules: the `component` shape, instance keys, the master, override targets, keys and
//! properties, the component-subtree restrictions, and the properties of expanded nodes.
use super::{scope::Resolver, Lookup};
use crate::{
    instances::{INSTANCE_KEYS, OVERRIDE_KEYS},
    *,
};
use std::collections::{BTreeMap, BTreeSet};

/// One authored node's instance checks, in `Candidate::validate_instances`' order.
/// `placed` holds the expansion of `node` when it is an instance (keyed by id).
pub(super) fn check_instance_node(
    lookup: &dyn Lookup,
    node: &Node,
    placed: &BTreeMap<String, Node>,
    themes: &BTreeSet<String>,
) -> Result<()> {
    if let Some(component) = node.props.get("component") {
        if component.as_object().is_none_or(|o| o.len() != 1)
            || component["name"]
                .as_str()
                .is_none_or(|s| s.is_empty() || s.len() > 256)
        {
            return Err(StorageError::validation(
                "props.component",
                "Component requires a name up to 256 bytes",
            ));
        }
    }
    if node.kind == NodeKind::Instance {
        check_instance(lookup, node, placed, themes)?;
    }
    if in_component(lookup, &node.id)
        && ["links", "animations", "flowStart"].iter().any(|key| {
            node.props
                .get(key)
                .is_some_and(|v| v.as_array().is_none_or(|a| !a.is_empty()))
        })
    {
        return Err(StorageError::validation(
            "props.component",
            "Component subtrees cannot carry links, flowStart or animations",
        ));
    }
    Ok(())
}

fn check_instance(
    lookup: &dyn Lookup,
    node: &Node,
    placed: &BTreeMap<String, Node>,
    themes: &BTreeSet<String>,
) -> Result<()> {
    let keys = node.props.as_object().ok_or_else(|| {
        StorageError::validation("props", "Instance properties must be an object")
    })?;
    for key in keys.keys() {
        if !INSTANCE_KEYS.contains(&key.as_str()) {
            return Err(key_error(
                &node.id,
                key,
                &format!("props.{key}"),
                INSTANCE_KEYS,
            ));
        }
    }
    let master = node.props["componentId"]
        .as_str()
        .and_then(|id| lookup.node(id))
        .filter(|n| n.kind == NodeKind::Frame && n.props.get("component").is_some())
        .ok_or_else(|| {
            StorageError::validation("props.componentId", "Expected a component frame")
        })?;
    let Some(value) = node.props.get("overrides") else {
        return Ok(());
    };
    let overrides = value.as_object().ok_or_else(|| {
        StorageError::validation("props.overrides", "Overrides must be an object")
    })?;
    let resolver = |theme| Resolver {
        lookup,
        overlay: Some(placed),
        theme,
    };
    // Override targets are authored master descendants: walk authored ancestors only.
    let plain = Resolver {
        lookup,
        overlay: None,
        theme: "default",
    };
    for (target, patch) in overrides {
        if target == &master.id || !plain.ancestors(target).any(|a| a.id == master.id) {
            return Err(not_descendant(lookup, &node.id, target, &master.id));
        }
        let patch = patch.as_object().ok_or_else(|| {
            StorageError::validation(
                &format!("props.overrides.{target}"),
                "Override must be a property object",
            )
        })?;
        for key in patch.keys() {
            if !OVERRIDE_KEYS.contains(&key.as_str()) {
                return Err(key_error(
                    &node.id,
                    key,
                    &format!("props.overrides.{target}.{key}"),
                    OVERRIDE_KEYS,
                ));
            }
        }
        let child = placed
            .get(&format!("{}/{target}", node.id))
            .expect("master descendant has an expanded placement");
        for theme in themes {
            resolver(theme).properties(child).map_err(|mut e| {
                e.field = Some(format!(
                    "props.overrides.{target}.{}",
                    e.field
                        .unwrap_or_else(|| "props".into())
                        .trim_start_matches("props.")
                ));
                e
            })?;
        }
    }
    Ok(())
}

/// Properties of expanded nodes (instance roots and virtual nodes) under every theme,
/// in the expander's order, with `placed` as the overlay over `lookup`.
pub(super) fn check_expanded<'a>(
    lookup: &dyn Lookup,
    placed: &BTreeMap<String, Node>,
    order: impl Iterator<Item = &'a Node> + Clone,
    themes: &BTreeSet<String>,
) -> Result<()> {
    for theme in themes {
        let context = Resolver {
            lookup,
            overlay: Some(placed),
            theme,
        };
        for node in order.clone() {
            context.properties(node)?;
        }
    }
    Ok(())
}

/// A non-descendant override target, most likely a mistyped descendant ID or instance key.
fn not_descendant(lookup: &dyn Lookup, node: &str, target: &str, master: &str) -> StorageError {
    let mut descendants = Vec::new();
    let mut pending = vec![master];
    while let Some(id) = pending.pop() {
        for child in lookup.children(id) {
            descendants.push(child.id.as_str());
            pending.push(&child.id);
        }
    }
    descendants.sort_unstable();
    let mut error = StorageError::validation(
        &format!("props.overrides.{target}"),
        "Override target must be a master descendant",
    );
    let near = crate::suggest_among(
        target,
        descendants.into_iter().chain(INSTANCE_KEYS.iter().copied()),
    );
    let context = error.node_context();
    context.node_id = Some(node.into());
    context.suggestion = near.map(Into::into);
    error
}

/// Whether `id` or one of its ancestors is a component master.
fn in_component(lookup: &dyn Lookup, id: &str) -> bool {
    let mut current = lookup.node(id);
    for _ in 0..128 {
        let Some(node) = current else {
            break;
        };
        if node.props.get("component").is_some() {
            return true;
        }
        current = node.parent_id.as_ref().and_then(|id| lookup.node(id));
    }
    false
}

fn key_error(node: &str, key: &str, field: &str, allowed: &[&str]) -> StorageError {
    let mut error = StorageError::validation(field, "Unsupported instance override or property");
    let context = error.node_context();
    context.node_id = Some(node.into());
    context.allowed_keys = Some(allowed.iter().map(|s| (*s).into()).collect());
    context.suggestion = crate::suggest_among(key, allowed.iter().copied()).map(Into::into);
    error
}
