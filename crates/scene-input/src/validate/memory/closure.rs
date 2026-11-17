//! Old/new ancestry and instance users drive invalidation; dependencies are
//! lookup-only support, not another whole-project instance expansion.
use crate::{read_scope::ReadView, Operation};
use std::collections::BTreeSet;

pub(super) fn affected(
    before: &ReadView,
    after: &ReadView,
    operations: &[Operation],
    changed: &[String],
) -> BTreeSet<String> {
    let mut pending = changed.to_vec();
    for op in operations {
        // Intermediate placements/references may disappear from BOTH endpoint
        // revisions. Prefix validation still needs their complete lookup scope.
        match op {
            Operation::CreateNode {
                parent_id, props, ..
            } => {
                pending.extend(parent_id.iter().cloned());
                pending.extend(props["componentId"].as_str().map(str::to_owned));
            }
            Operation::ReparentNode { parent_id, .. } => pending.push(parent_id.clone()),
            Operation::UpdateNode { props, .. } => {
                pending.extend(props["componentId"].as_str().map(str::to_owned));
            }
            _ => {}
        }
        let id = match op {
            Operation::CreateNode { id, .. }
            | Operation::ReparentNode { id, .. }
            | Operation::ReorderNode { id, .. }
            | Operation::DeleteSubtree { id } => id,
            _ => continue,
        };
        for view in [before, after] {
            if let Some(parent) = view.node(id).and_then(|n| n.parent_id.as_ref()) {
                // Complete siblings/subtree at placement parents, including
                // unchanged siblings, are required for Candidate::place parity.
                pending.push(parent.clone());
            }
        }
    }
    let mut selected = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !selected.insert(id.clone()) {
            continue;
        }
        for view in [before, after] {
            pending.extend(view.children(&id).map(str::to_owned));
            let mut cursor = view.node(&id);
            for _ in 0..128 {
                let Some(node) = cursor else {
                    break;
                };
                if node.props.get("component").is_some() {
                    for users in [before, after] {
                        pending.extend(users.component_users(&node.id).map(str::to_owned));
                    }
                }
                cursor = node.parent_id.as_deref().and_then(|id| view.node(id));
            }
        }
    }
    selected
}

pub(super) fn support(
    before: &ReadView,
    after: &ReadView,
    affected: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut selected = affected.clone();
    let mut scanned = BTreeSet::new();
    loop {
        let pending: Vec<_> = selected.difference(&scanned).cloned().collect();
        if pending.is_empty() {
            return selected;
        }
        for id in pending {
            scanned.insert(id.clone());
            for view in [before, after] {
                let Some(node) = view.node(&id) else {
                    continue;
                };
                let mut cursor = Some(node);
                for _ in 0..128 {
                    let Some(node) = cursor else {
                        break;
                    };
                    selected.insert(node.id.clone());
                    cursor = node.parent_id.as_deref().and_then(|id| view.node(id));
                }
                if let Some(master) = node.props["componentId"].as_str() {
                    let mut descendants = vec![master.to_owned()];
                    let mut seen = BTreeSet::new();
                    while let Some(id) = descendants.pop() {
                        if !seen.insert(id.clone()) {
                            continue;
                        }
                        selected.insert(id.clone());
                        descendants.extend(view.children(&id).map(str::to_owned));
                    }
                }
                for link in crate::node_links(&node.props) {
                    if let Some(target) = link.to {
                        selected.insert(target);
                    }
                }
            }
        }
    }
}
