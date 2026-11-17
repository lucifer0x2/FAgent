//! Authored outline/subtree output borrows the persistent tree. Only instance
//! child counts require an output-specific expansion; ordinary nodes are not copied.
use super::{index::TreeIndex, instances, pagination, ReadRequest};
use crate::error::{ApiError, Result};
use fagent_storage::{
    read_scope::{ReadScope, ScopedSnapshot},
    NodeKind,
};
use serde_json::{json, Value};

pub(super) fn read(
    scoped: &ScopedSnapshot<'_>,
    name: &str,
    req: ReadRequest,
    scope: String,
    offset: usize,
) -> Result<Value> {
    let root = req.node_id.as_deref().or(req.frame_id.as_deref());
    if name == "get_subtree" && req.node_id.is_none() {
        return Err(ApiError::invalid(
            "A subtree requires nodeId; context requires frameId",
        ));
    }
    if req.node_id.is_some() && req.frame_id.is_some() {
        return Err(ApiError::invalid("Choose one scope ID"));
    }
    let index = TreeIndex::scoped(scoped);
    if root.is_some_and(|id| index.get(id).is_none()) {
        return Err(ApiError::invalid("Scope node does not exist"));
    }
    let ordered = index.preorder(root);
    let mut entries = Vec::new();
    for (node, depth) in ordered
        .iter()
        .filter(|(_, d)| req.depth.is_none_or(|max| *d <= max))
    {
        let mut value = if name == "get_outline" {
            json!({"id":node.id,"parentId":node.parent_id,"kind":node.kind,"name":node.props["name"],"sortKey":node.sort_key})
        } else {
            let mut value = json!(node);
            if let Some(props) = &req.properties {
                value["props"]
                    .as_object_mut()
                    .unwrap()
                    .retain(|k, _| props.contains(k));
            }
            value
        };
        if node.kind == NodeKind::Instance {
            let (expanded, _) = scoped.expand_nodes(ReadScope::Target(&node.id))?;
            instances::reference(node, &mut value, &TreeIndex::new(&expanded), None);
        }
        let children = index.child_count(&node.id);
        if req.depth == Some(*depth) && children > 0 {
            value["omittedChildren"] = json!(children);
        }
        entries.push(("nodes".into(), value));
    }
    let s = scoped.snapshot();
    let response = json!({"schemaVersion":1,"projectId":s.project_id,"revision":s.revision,"scope":root,"nodes":[],"limits":{"depth":req.depth,"nodes":req.limit.unwrap_or(200),"bytes":req.budget.unwrap_or(65536)}});
    pagination::page(
        response,
        entries,
        offset,
        req.limit.unwrap_or(200),
        req.budget.unwrap_or(65536),
        &scope,
    )
}
