//! Structural validation shared by load and writes. Error codes, fields and messages
//! follow `Candidate::apply` in `fagent-scene-input`; property validation is not
//! run here.
use crate::props::{component_id, is_master};
use crate::store::{Key, Store};
use fagent_scene_input::validate::{structural, valid_id};
use fagent_scene_input::{Node, NodeKind, Result, StorageError};

/// Maximum chain length (node plus ancestors), as `Candidate` checks it.
pub(crate) const MAX_DEPTH: usize = 128;

fn readonly(instance: &str) -> StorageError {
    StorageError::new(
        "INSTANCE_CHILD_READONLY",
        &format!("Virtual children are read-only; update_node {instance} props.overrides"),
    )
}

/// Ids with `/` name virtual instance children, which are never authored. Checks the
/// operation's node id, then its parent id (`instance_write_guard`).
pub(crate) fn not_virtual(id: &str, parent: Option<&str>) -> Result<()> {
    match id.split_once('/').or_else(|| parent?.split_once('/')) {
        Some((instance, _)) => Err(readonly(instance)),
        None => Ok(()),
    }
}

/// A valid authored id (`values::id`).
pub(crate) fn valid(id: &str) -> Result<()> {
    match valid_id(id) {
        true => Ok(()),
        false => Err(StorageError::validation(
            "id",
            "ID must contain 1–128 ASCII letters, digits, underscores or hyphens",
        )),
    }
}

pub(crate) fn too_many(max_nodes: usize) -> StorageError {
    StorageError::validation(
        "nodes",
        &format!("Project exceeds {max_nodes} authored nodes"),
    )
}

pub(crate) fn missing(id: &str) -> StorageError {
    structural::missing_node(id)
}

/// An existing node.
pub(crate) fn existing(store: &Store, id: &str) -> Result<Key> {
    store.key(id).ok_or_else(|| missing(id))
}

fn pages_are_roots() -> StorageError {
    StorageError::validation("parentId", "Pages must be roots")
}

pub(crate) fn cycle(id: &str, parent: &str) -> StorageError {
    structural::cycle(id, parent)
}

/// A cycle found while loading stored rows, not caused by one operation.
pub(crate) fn stored_cycle() -> StorageError {
    StorageError::validation("parentId", "Tree cycle detected")
}

/// The parent a node `id` of `kind` may have: none for pages, an existing container
/// otherwise. A node naming itself as parent fails as `Candidate` sees it: a cycle when
/// it is a container, a non-container parent otherwise.
pub(crate) fn parent(
    store: &Store,
    id: &str,
    kind: &NodeKind,
    parent: Option<&str>,
) -> Result<Option<Key>> {
    match (kind, parent) {
        (NodeKind::Page, None) => Ok(None),
        (NodeKind::Page, Some(_)) => Err(pages_are_roots()),
        (_, None) => Err(StorageError::validation(
            "parentId",
            "Non-page nodes require a parent",
        )),
        (_, Some(p)) if p == id => match kind.container() {
            true => Err(cycle(id, p)),
            false => Err(structural::not_container(id, p, kind.as_str())),
        },
        (_, Some(p)) => {
            let key = store
                .key(p)
                .ok_or_else(|| structural::missing_parent(id, p))?;
            let parent_kind = &store.node(key).kind;
            match parent_kind.container() {
                true => Ok(Some(key)),
                false => Err(structural::not_container(id, p, parent_kind.as_str())),
            }
        }
    }
}

/// Fails when a node at `depth` (1 for a page) exceeds the maximum depth.
pub(crate) fn depth(depth: usize) -> Result<()> {
    match depth > MAX_DEPTH {
        true => Err(StorageError::validation(
            "parentId",
            "Tree exceeds maximum depth 128",
        )),
        false => Ok(()),
    }
}

/// Chain length of `key` (itself plus ancestors), counting at most `MAX_DEPTH + 1`.
pub(crate) fn depth_of(store: &Store, key: Key) -> usize {
    1 + store.ancestors(key).take(MAX_DEPTH).count()
}

/// Levels below `key` in its deepest branch (0 for a leaf).
pub(crate) fn height(store: &Store, key: Key) -> usize {
    let mut deepest = 0;
    let mut stack = vec![(key, 0)];
    while let Some((key, depth)) = stack.pop() {
        deepest = deepest.max(depth);
        let children = store.slot(key).children.iter();
        stack.extend(children.map(|&child| (child, depth + 1)));
    }
    deepest
}

/// For an instance: its `componentId` must name a component master. Reported on the
/// node like `Candidate`'s property validation.
pub(crate) fn instance_target(store: &Store, node: &Node) -> Result<()> {
    if node.kind != NodeKind::Instance {
        return Ok(());
    }
    let found = component_id(&node.props)
        .and_then(|id| store.get(id))
        .is_some_and(|slot| is_master(&slot.node.kind, &slot.node.props));
    match found {
        true => Ok(()),
        false => Err(
            StorageError::validation("props.componentId", "Expected a component frame")
                .on_node(&node.id, node.kind.as_str()),
        ),
    }
}

/// A master would lose its component status or be deleted while instances use it.
/// Instance ids are sorted; at most 20 are listed, with the full count.
pub(crate) fn component_in_use(mut users: Vec<String>) -> StorageError {
    users.sort();
    let mut error = StorageError::new(
        "COMPONENT_IN_USE",
        "Delete dependent instances before deleting their component",
    );
    error.node_context().instance_ids = Some(users.iter().take(20).cloned().collect());
    error.node_context().count = Some(users.len());
    error
}

/// Load-time check of a stored node's parent (same rules as writes).
pub(crate) fn stored_parent(store: &Store, node: &Node) -> Result<Option<Key>> {
    parent(store, &node.id, &node.kind, node.parent_id.as_deref())
}
