//! Override targets of one instance: a master descendant ID, or a path through nested
//! instances (`btn/label`: `btn` is an instance inside the master, `label` a descendant of
//! `btn`'s master). Each segment is checked against the authored master it names, so a
//! target that a master edit leaves dangling fails on the next commit of that edit.
use super::{scope::Resolver, Lookup};
use crate::{instances::INSTANCE_KEYS, *};

/// Checks that `target` names a descendant of `master`, segment by segment.
pub(super) fn check_target(
    lookup: &dyn Lookup,
    node: &str,
    target: &str,
    master: &Node,
) -> Result<()> {
    // Override targets are authored master descendants: walk authored ancestors only.
    let plain = Resolver {
        lookup,
        overlay: None,
        theme: "default",
    };
    let segments: Vec<&str> = target.split('/').collect();
    let mut current = master;
    for (index, segment) in segments.iter().enumerate() {
        let walked = segments[..index].join("/");
        if index == 0 && *segment == current.id {
            return Err(master_root_target(node, target));
        }
        if !plain.ancestors(segment).any(|a| a.id == current.id) {
            return Err(not_descendant(
                lookup, node, target, &walked, segment, current,
            ));
        }
        if index + 1 == segments.len() {
            break;
        }
        current = lookup
            .node(segment)
            .filter(|n| n.kind == NodeKind::Instance)
            .and_then(|n| n.props["componentId"].as_str())
            .and_then(|id| lookup.node(id))
            .filter(|n| n.kind == NodeKind::Frame && n.props.get("component").is_some())
            .ok_or_else(|| not_instance(node, target, segment))?;
    }
    Ok(())
}

/// An override keyed by the master root: its props are instance keys, not overrides.
fn master_root_target(node: &str, target: &str) -> StorageError {
    let mut error = StorageError::validation(
        &format!("props.overrides.{target}"),
        &format!(
            "Override targets are master descendants; the master root's properties are set on the instance itself (for example {{\"componentId\":\"{target}\",\"fill\":\"#ffffff\"}}). Instance keys: {}",
            INSTANCE_KEYS.join(", ")
        ),
    );
    let context = error.node_context();
    context.node_id = Some(node.into());
    context.allowed_keys = Some(INSTANCE_KEYS.iter().map(|s| (*s).into()).collect());
    error
}

/// A segment that is not a descendant of the master it is read in, most likely a mistyped
/// descendant ID or instance key; the suggestion keeps the path walked so far.
fn not_descendant(
    lookup: &dyn Lookup,
    node: &str,
    target: &str,
    walked: &str,
    segment: &str,
    master: &Node,
) -> StorageError {
    let mut descendants = lookup.descendants(&master.id);
    descendants.sort_unstable();
    let message = match walked {
        "" => "Override target must be a master descendant".to_owned(),
        _ => format!(
            "Override path segment `{segment}` must be a descendant of master `{}`, which `{walked}` uses",
            master.id
        ),
    };
    let mut error = StorageError::validation(&format!("props.overrides.{target}"), &message);
    let near = match walked {
        "" => crate::suggest_among(
            segment,
            descendants.into_iter().chain(INSTANCE_KEYS.iter().copied()),
        )
        .map(str::to_owned),
        _ => crate::suggest_among(segment, descendants).map(|near| format!("{walked}/{near}")),
    };
    let context = error.node_context();
    context.node_id = Some(node.into());
    context.suggestion = near;
    error
}

/// A path segment followed by more segments must be an instance of a component.
fn not_instance(node: &str, target: &str, segment: &str) -> StorageError {
    let mut error = StorageError::validation(
        &format!("props.overrides.{target}"),
        &format!(
            "Override path segment `{segment}` is not an instance; only a nested instance's ID may be followed by /<descendantId>"
        ),
    );
    error.node_context().node_id = Some(node.into());
    error
}
