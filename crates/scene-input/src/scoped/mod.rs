//! Bounded reads over an immutable, explicitly validated authored revision.
mod closure;
mod guard;
mod index;
mod input;
mod navigation;
pub(crate) mod persistent;
mod snapshot_source;
mod traversal;
mod view;
mod view_reads;
#[cfg(test)]
mod view_tests;

pub use crate::validate::resource_builds;
use crate::{
    validate::{Candidate, ScopedResources},
    Node, Note, Result, Snapshot,
};
pub use index::ScopeIndex;
pub use snapshot_source::references_of;
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};
pub use view::{ReadView, ViewSource};

#[derive(Clone, Copy, Debug)]
pub enum ReadScope<'a> {
    Page(&'a str),
    Target(&'a str),
}

/// A borrowed read capability. Standalone/untrusted callers must use `validate`.
pub struct ScopedSnapshot<'a> {
    snapshot: &'a Snapshot,
    index: Arc<ScopeIndex>,
    /// Resource maps/themes, built on first use and shared by every resolve.
    resources: OnceLock<Result<ScopedResources>>,
    view: Option<&'a ReadView>,
}

/// Source-order authored/expanded vectors, with ancestors needed for placed themes.
/// Definitions are included only when requested. Resources remain on the snapshot.
pub struct ScopedNodes<'a> {
    pub authored: Vec<&'a Node>,
    pub dependencies: Vec<&'a Node>,
    pub expanded: Vec<Node>,
    pub stats: ScopeStats,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScopeStats {
    pub closure_nodes: usize,
    pub expanded_nodes: usize,
    pub resolved_nodes: usize,
}

impl<'a> ScopedSnapshot<'a> {
    pub fn validate(snapshot: &'a Snapshot) -> Result<Self> {
        Candidate::from_snapshot(snapshot).validate()?;
        Ok(Self::from_validated(
            snapshot,
            Arc::new(ScopeIndex::new(snapshot)),
        ))
    }

    /// Explicit trusted boundary for storage adapters with a validated-write invariant.
    /// The caller guarantees the *entire* snapshot already passed full validation
    /// and that `index` was built with `ScopeIndex::new` for this exact immutable
    /// snapshot, including its node and note vectors in source order.
    /// Cache identity alone does not establish either guarantee. This deliberately
    /// does not change standalone resolution/validation APIs. An index bound to a
    /// different snapshot is never dereferenced: it is rebuilt for this one.
    pub fn from_validated(snapshot: &'a Snapshot, index: Arc<ScopeIndex>) -> Self {
        let index = if index.indexes(snapshot) {
            index
        } else {
            Arc::new(ScopeIndex::new(snapshot))
        };
        Self {
            snapshot,
            index,
            resources: OnceLock::new(),
            view: None,
        }
    }

    fn from_view(view: &'a ReadView) -> Self {
        Self {
            snapshot: &view.snapshot,
            index: Arc::default(),
            resources: OnceLock::new(),
            view: Some(view),
        }
    }

    pub fn snapshot(&self) -> &'a Snapshot {
        self.snapshot
    }

    /// Authored node count, including persistent views whose header has no nodes.
    pub fn node_count(&self) -> usize {
        self.view.map_or(self.snapshot.nodes.len(), ReadView::len)
    }

    pub fn node(&self, id: &str) -> Option<&'a Node> {
        if let Some(view) = self.view {
            return view.node(id);
        }
        self.index
            .positions
            .get(id)
            .map(|&i| &self.snapshot.nodes[i])
    }

    /// Top-level authored roots in source order. Apply stable sibling sortKey
    /// ordering at the read layer, exactly as for the full source vector.
    pub fn roots(&self) -> Box<dyn Iterator<Item = &'a Node> + '_> {
        match self.view {
            Some(view) => view.source.roots(),
            None => Box::new(self.index.roots.iter().map(|&i| &self.snapshot.nodes[i])),
        }
    }

    /// Authored frames in snapshot source order, including nested frames.
    pub fn frames(&self) -> Box<dyn Iterator<Item = &'a Node> + '_> {
        match self.view {
            Some(view) => Box::new(view.frames().into_iter()),
            None => Box::new(self.index.frames.iter().map(|&i| &self.snapshot.nodes[i])),
        }
    }

    /// Immediate authored children in source order, without expanding a page.
    pub fn children(&self, id: &str) -> Box<dyn Iterator<Item = &'a Node> + '_> {
        if let Some(view) = self.view {
            return view.source.children(id);
        }
        Box::new(
            self.index
                .children
                .get(id)
                .into_iter()
                .flatten()
                .map(|&i| &self.snapshot.nodes[i]),
        )
    }

    /// Notes anchored to this authored dependency closure, in snapshot note order.
    /// Include `ScopedNodes::dependencies` for intent on virtual source/master IDs.
    pub fn notes_for_nodes<'id>(&self, ids: impl IntoIterator<Item = &'id str>) -> Vec<&'a Note> {
        if let Some(view) = self.view {
            let notes: BTreeMap<&str, &Note> = ids
                .into_iter()
                .flat_map(|id| view.source.notes_on(id))
                .map(|note| (note.id.as_str(), note))
                .collect();
            return notes.into_values().collect();
        }
        let positions: std::collections::BTreeSet<_> = ids
            .into_iter()
            .flat_map(|id| self.index.notes.get(id).into_iter().flatten().copied())
            .collect();
        positions
            .into_iter()
            .map(|i| &self.snapshot.notes[i])
            .collect()
    }

    fn resources(&self) -> Result<&ScopedResources> {
        self.resources
            .get_or_init(|| ScopedResources::new(self.snapshot))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// Requested-theme validity, with the same error as resolution, without
    /// resolving any node or token value.
    pub fn check_theme(&self, theme: &str) -> Result<()> {
        self.resources()?.check_theme(theme)
    }

    /// Resolve only these expanded nodes, reusing full immutable resource metadata.
    /// The vector retains expansion order, IDs, visibility and placed-theme semantics.
    pub fn resolve(&self, nodes: &mut ScopedNodes<'_>, theme: &str) -> Result<Vec<Node>> {
        let result = self.resources()?.resolve(&nodes.expanded, theme)?;
        nodes.stats.resolved_nodes = result.len();
        Ok(result)
    }

    /// Full token resource responses retain their existing contract without node work.
    pub fn resolved_tokens(&self, theme: &str) -> Result<BTreeMap<String, serde_json::Value>> {
        self.resources()?.tokens(theme)
    }
}
