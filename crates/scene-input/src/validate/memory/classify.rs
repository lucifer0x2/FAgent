//! Safe local operation classes. Global changes retain the reference validator.
use crate::{read_scope::ReadView, NodeKind, Operation};

pub(super) fn supports(before: &ReadView, operations: &[Operation]) -> bool {
    operations.iter().all(|op| {
        let (id, parent, props) = match op {
            Operation::CreateNode {
                id,
                kind,
                parent_id,
                props,
                ..
            } => {
                if *kind == NodeKind::Page {
                    return false;
                }
                (id, parent_id.as_deref(), Some(props))
            }
            Operation::UpdateNode { id, props, .. } => (id, None, Some(props)),
            Operation::ReparentNode { id, parent_id, .. } => (id, Some(parent_id.as_str()), None),
            Operation::ReorderNode { id, .. } | Operation::DeleteSubtree { id } => (id, None, None),
            Operation::UpsertToken { .. }
            | Operation::UpsertStyle { .. }
            | Operation::CreateNote { .. }
            | Operation::ReplyNote { .. }
            | Operation::UpdateNote { .. }
            | Operation::DeleteNote { .. } => return false,
        };
        if props.is_some_and(|p| {
            ["theme", "links", "flowStart", "animations"]
                .iter()
                .any(|k| p.get(k).is_some())
        }) {
            return false;
        }
        if before.node(id).is_some_and(|n| n.kind == NodeKind::Page) {
            return false;
        }
        // Structural root edits can alter default navigation and global order.
        // A plain root/master property edit is still fully local.
        if !matches!(op, Operation::UpdateNode { .. }) {
            for parent in parent
                .into_iter()
                .chain(before.node(id).and_then(|n| n.parent_id.as_deref()))
            {
                if before
                    .node(parent)
                    .is_some_and(|n| n.kind == NodeKind::Page)
                {
                    return false;
                }
            }
        }
        true
    })
}
