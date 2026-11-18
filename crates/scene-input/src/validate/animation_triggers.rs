//! Hover sources are authored tree references, checked on the final batch candidate.
use super::{scope::Resolver, Candidate};
use crate::{error::*, model::*};
use serde_json::Value;

impl Candidate {
    pub(super) fn validate_animation_triggers(&self) -> Result<()> {
        let resolver = self.resolver();
        for target in self.nodes.values() {
            check_triggers(&resolver, target)?;
        }
        Ok(())
    }
}

/// Every `animations[].triggerNodeId` of `target` names an authored group or frame that
/// is a strict ancestor of it in the same root frame.
pub(super) fn check_triggers(resolver: &Resolver<'_>, target: &Node) -> Result<()> {
    let Some(animations) = target.props.get("animations").and_then(Value::as_array) else {
        return Ok(());
    };
    for (i, animation) in animations.iter().enumerate() {
        // Literal shape and on:"hover" are checked with the other animation keys.
        let Some(source) = animation.get("triggerNodeId").and_then(Value::as_str) else {
            continue;
        };
        let mut ancestors = resolver.ancestors(&target.id);
        let valid = source != target.id
            && ancestors
                .find(|node| node.id == source)
                .is_some_and(|node| {
                    matches!(node.kind, NodeKind::Group | NodeKind::Frame)
                        && (resolver.is_root_frame(source)
                            || ancestors.any(|node| resolver.is_root_frame(&node.id)))
                });
        if !valid {
            return Err(StorageError::validation(
                &format!("props.animations[{i}].triggerNodeId"),
                "triggerNodeId must identify an authored group or frame that is a strict ancestor in the same root frame",
            )
            .on_node(&target.id, target.kind.as_str()));
        }
    }
    Ok(())
}
