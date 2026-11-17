//! Operation builders for tests.
use fagent_node_memory::Operation;
use fagent_scene_input::NodeKind;
use serde_json::{json, Value};

pub fn new(id: &str, kind: NodeKind, parent: Option<&str>, props: Value) -> Operation {
    Operation::CreateNode {
        id: id.into(),
        kind,
        parent_id: parent.map(Into::into),
        props,
        index: None,
    }
}

pub fn create(id: &str, kind: NodeKind, parent: &str, props: Value) -> Operation {
    new(id, kind, Some(parent), props)
}

pub fn create_at(id: &str, kind: NodeKind, parent: &str, index: usize) -> Operation {
    Operation::CreateNode {
        id: id.into(),
        kind,
        parent_id: Some(parent.into()),
        props: json!({}),
        index: Some(index),
    }
}

pub fn update(id: &str, props: Value, merge: bool) -> Operation {
    Operation::UpdateNode {
        id: id.into(),
        props,
        merge,
    }
}

pub fn reparent(id: &str, parent: &str) -> Operation {
    reparent_at(id, parent, None)
}

pub fn reparent_at(id: &str, parent: &str, index: Option<usize>) -> Operation {
    Operation::ReparentNode {
        id: id.into(),
        parent_id: parent.into(),
        index,
    }
}

pub fn reorder(id: &str, index: usize) -> Operation {
    Operation::ReorderNode {
        id: id.into(),
        index,
    }
}

pub fn delete(id: &str) -> Operation {
    Operation::DeleteSubtree { id: id.into() }
}
