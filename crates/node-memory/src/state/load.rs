//! Bulk load of a snapshot: one pass to allocate, one to link, one sort per sibling
//! list and one top-down walk for derived indexes. Structural checks only: property
//! validation (props shape and instance targets included) is the engine's admission,
//! which never fails a load, so a batch that deletes or avoids a corrupt node commits.
use crate::checks;
use crate::store::{Key, Store};
use fagent_scene_input::{Node, Result, StorageError};

/// Builds a store from authored nodes, moving them in without cloning.
pub(crate) fn build(nodes: Vec<Node>, max_nodes: usize) -> Result<Store> {
    if nodes.len() > max_nodes {
        return Err(checks::too_many(max_nodes));
    }
    let mut store = Store::default();
    store.reserve(nodes.len());
    for node in nodes {
        let on_node = |e: StorageError| e.on_node(&node.id, node.kind.as_str());
        checks::not_virtual(&node.id, None).map_err(on_node)?;
        checks::valid(&node.id).map_err(on_node)?;
        if store.contains(&node.id) {
            return Err(on_node(StorageError::validation("id", "Duplicate node ID")));
        }
        store.load_node(node);
    }
    let keys: Vec<Key> = store.iter().map(|(key, _)| key).collect();
    for &key in &keys {
        let node = store.node(key);
        let parent = checks::stored_parent(&store, node)
            .map_err(|e| e.on_node(&node.id, node.kind.as_str()))?;
        store.load_link(parent, key);
    }
    store.sort_all();
    index(&mut store)?;
    Ok(store)
}

/// Fills derived values top-down from the pages and checks the maximum depth. Nodes
/// not reached from a page sit on a parent cycle.
fn index(store: &mut Store) -> Result<()> {
    let mut stack: Vec<(Key, usize)> = store.children(None).iter().map(|&k| (k, 1)).collect();
    let mut reached = 0;
    while let Some((key, depth)) = stack.pop() {
        reached += 1;
        let node = store.node(key);
        checks::depth(depth).map_err(|e| e.on_node(&node.id, node.kind.as_str()))?;
        let derived = store.derive(key);
        store.init(key, derived);
        let children = store.slot(key).children.iter();
        stack.extend(children.map(|&child| (child, depth + 1)));
    }
    if reached != store.len() {
        return Err(checks::stored_cycle());
    }
    Ok(())
}
