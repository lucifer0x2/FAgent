//! Counters the delta rules keep between revisions: the expanded size of the project,
//! so the expanded-node limit is checked without expanding every instance.
use super::expand::{expand, extras};
use crate::read_scope::persistent::Map;
use crate::validate::Lookup;
use crate::{NodeKind, StorageError};

/// Expanded size of one revision: virtual nodes per authored instance and the total
/// (authored nodes plus virtual nodes). Cloning is cheap; an update copies one shard.
#[derive(Clone, Default)]
pub struct Facts {
    extra: Map<usize>,
    total: usize,
}

impl Facts {
    /// Authored plus virtual nodes.
    pub fn total(&self) -> usize {
        self.total
    }

    pub fn estimated_bytes(&self) -> usize {
        self.extra.estimated_bytes()
    }

    /// The facts after a batch: `instances` lists every node whose expansion may have
    /// changed (deleted ones included) and `expanded` is the new expansion of the ones
    /// that are instances now. Fails when the expanded project exceeds `max_nodes`.
    pub(super) fn update<'a>(
        &self,
        authored: (usize, usize),
        instances: impl Iterator<Item = &'a str>,
        expanded: &[crate::Node],
        max_nodes: usize,
    ) -> crate::Result<Self> {
        let (before, after) = authored;
        let mut next = self.clone();
        next.total = (next.total + after).saturating_sub(before);
        for id in instances {
            if let Some(&old) = next.extra.get(id) {
                next.total = next.total.saturating_sub(old);
                next.extra.remove(id);
            }
        }
        for (root, count) in extras(expanded) {
            next.total = next.total.saturating_add(count);
            next.extra.insert(root.to_owned(), count);
        }
        if next.total > max_nodes {
            return Err(StorageError::new(
                "INSTANCE_LIMIT",
                &format!("Expanded project exceeds {max_nodes} nodes"),
            ));
        }
        Ok(next)
    }
}

/// The facts of a whole revision, given every authored instance id. An instance that
/// does not expand (stored data admitted although invalid) counts no virtual nodes.
pub fn facts<'a>(lookup: &dyn Lookup, instances: impl Iterator<Item = &'a str>) -> Facts {
    let mut facts = Facts {
        total: lookup.node_count(),
        ..Facts::default()
    };
    for node in instances.filter_map(|id| lookup.node(id)) {
        if node.kind != NodeKind::Instance {
            continue;
        }
        let Ok(expanded) = expand(lookup, &[node]) else {
            continue;
        };
        for (root, count) in extras(&expanded) {
            facts.total += count;
            facts.extra.insert(root.to_owned(), count);
        }
    }
    facts
}
