//! The only module that reads or writes node props. Props stay the stored JSON `Value`
//! for now; typed props can replace this module without touching the indexes.
use fagent_scene_input::{merge_props, NodeKind, Result, StorageError};
use serde_json::{Map, Value};

/// A component master: a frame carrying `props.component`.
pub(crate) fn is_master(kind: &NodeKind, props: &Value) -> bool {
    *kind == NodeKind::Frame
        && fagent_scene_input::NodeProps::new(props)
            .component()
            .is_some()
}

/// The master id an instance points at, if any.
pub(crate) fn component_id(props: &Value) -> Option<&str> {
    fagent_scene_input::NodeProps::new(props).component_id()
}

/// The `to` ids of `props.links` entries (the shape `prune_links` and readers use).
pub(crate) fn link_targets(props: &Value) -> impl Iterator<Item = &str> {
    fagent_scene_input::NodeProps::new(props).link_targets()
}

/// Props must be a JSON object.
pub(crate) fn check_object(props: &Value) -> Result<()> {
    match props.is_object() {
        true => Ok(()),
        false => Err(StorageError::validation(
            "props",
            "Properties must be an object",
        )),
    }
}

/// `update_node` props: each top-level key of `patch` replaces the stored value, or
/// with `merge` the patch is applied as an RFC 7386 merge patch (`merge_props`).
pub(crate) fn updated(stored: &Value, patch: &Map<String, Value>, merge: bool) -> Result<Value> {
    let mut next: Map<String, Value> = stored
        .as_object()
        .cloned()
        .ok_or_else(|| StorageError::validation("props", "Properties must be an object"))?;
    match merge {
        true => merge_props(&mut next, patch),
        false => next.extend(patch.clone()),
    }
    Ok(Value::Object(next))
}

/// Removes link entries whose `to` is in `gone`, dropping an emptied `links` key, as
/// `Candidate::prune_links` does. `None` when nothing changes.
pub(crate) fn pruned_links(props: &Value, gone: impl Fn(&str) -> bool) -> Option<Value> {
    let links = props.get("links")?.as_array()?;
    let kept: Vec<Value> = links
        .iter()
        .filter(|l| l["to"].as_str().is_none_or(|to| !gone(to)))
        .cloned()
        .collect();
    if kept.len() == links.len() {
        return None;
    }
    let mut next = props.as_object()?.clone();
    match kept.is_empty() {
        true => drop(next.remove("links")),
        false => drop(next.insert("links".into(), Value::Array(kept))),
    }
    Some(Value::Object(next))
}

/// Per-allocation allocator overhead (header and size-class rounding), approximate.
pub(crate) const ALLOC: usize = 16;
const VALUE: usize = std::mem::size_of::<Value>();
/// One `BTreeMap<String, Value>` node: 11 key and value slots plus header.
const MAP_NODE: usize = 16 + 11 * (std::mem::size_of::<String>() + VALUE);

/// Approximate heap bytes of a JSON value (its inline `Value` lives in the slot).
/// Uses lengths, not capacities, so equal values always estimate equally.
pub(crate) fn heap_bytes(value: &Value) -> usize {
    match value {
        Value::String(s) if !s.is_empty() => s.len() + ALLOC,
        Value::Array(items) if !items.is_empty() => {
            items.len() * VALUE + ALLOC + items.iter().map(heap_bytes).sum::<usize>()
        }
        Value::Object(map) if !map.is_empty() => {
            // Up to 11 entries share one node; larger maps run about 2/3 full.
            let nodes = if map.len() <= 11 {
                1
            } else {
                map.len().div_ceil(8)
            };
            let entries: usize = map
                .iter()
                .map(|(k, v)| k.len() + ALLOC + heap_bytes(v))
                .sum();
            nodes * (MAP_NODE + ALLOC) + entries
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn update_modes() {
        let stored = json!({"a":{"x":1,"y":2},"b":1});
        let patch = json!({"a":{"x":3},"b":null});
        let patch = patch.as_object().unwrap();
        let assign = updated(&stored, patch, false).unwrap();
        assert_eq!(assign, json!({"a":{"x":3},"b":null}));
        let merged = updated(&stored, patch, true).unwrap();
        assert_eq!(merged, json!({"a":{"x":3,"y":2}}));
        assert!(updated(&json!(1), patch, false).is_err());
    }

    #[test]
    fn master_component_and_links() {
        let master = json!({"component":{"name":"C"}});
        assert!(is_master(&NodeKind::Frame, &master));
        assert!(!is_master(&NodeKind::Group, &master));
        assert_eq!(component_id(&json!({"componentId":"m"})), Some("m"));
        assert!(heap_bytes(&master) > MAP_NODE);
        assert_eq!(heap_bytes(&json!({})), 0);
        let props = json!({"links":[{"to":"a"},{"to":"b"},"x",{"on":"click"}]});
        assert_eq!(link_targets(&props).collect::<Vec<_>>(), ["a", "b"]);
        let pruned = pruned_links(&props, |id| id == "a").unwrap();
        assert_eq!(pruned, json!({"links":[{"to":"b"},"x",{"on":"click"}]}));
        assert_eq!(pruned_links(&props, |id| id == "z"), None);
        let single = json!({"links":[{"to":"a"}],"name":"n"});
        assert_eq!(
            pruned_links(&single, |_| true).unwrap(),
            json!({"name":"n"})
        );
    }
}
