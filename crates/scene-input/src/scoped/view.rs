//! Immutable whole-project read revisions. A [`ReadView`] wraps one [`ViewSource`]: the
//! node engine's persistent state for HOT projects (shared with the writer, never
//! copied per commit), or a [`ReadView::new`] snapshot index for everything else.
use super::{snapshot_source::SnapshotSource, ScopedSnapshot};
use crate::{Node, NodeKind, Note, Snapshot};
use std::sync::{Arc, OnceLock};

/// Read access to one immutable authored revision. Ids are authored node ids; every
/// list is complete for that revision.
pub trait ViewSource: Send + Sync {
    /// Authored node count.
    fn node_count(&self) -> usize;
    fn node(&self, id: &str) -> Option<&Node>;
    fn note(&self, id: &str) -> Option<&Note>;
    /// Direct children in stored sibling order `(sort_key, id)`; empty when unknown.
    fn children(&self, id: &str) -> Box<dyn Iterator<Item = &Node> + '_>;
    fn child_count(&self, id: &str) -> usize;
    /// Pages in stored sibling order.
    fn roots(&self) -> Box<dyn Iterator<Item = &Node> + '_>;
    /// Every node, in no defined order.
    fn nodes(&self) -> Box<dyn Iterator<Item = &Node> + '_>;
    /// Every note, by id.
    fn notes(&self) -> Box<dyn Iterator<Item = &Note> + '_>;
    /// The page containing `id` (itself for a page).
    fn page(&self, id: &str) -> Option<&str>;
    /// Notes anchored to node `id`, by note id.
    fn notes_on(&self, id: &str) -> Vec<&Note>;
    /// Instances whose `props.componentId` is `master`, sorted by id.
    fn component_users(&self, master: &str) -> Vec<&str>;
    /// Nodes with a `props.links[].to` naming `target`, sorted by id.
    fn linking(&self, target: &str) -> Vec<&str>;
    /// Nodes whose props reference `key` (`token:`, `style:`, `font:`, `asset:`,
    /// `component:` or `link:` + id, nested overrides included), sorted by id.
    fn reference_users(&self, key: &str) -> Vec<&str>;
    /// Approximate retained bytes of the nodes, notes and indexes.
    fn estimated_bytes(&self) -> usize;
    /// The expanded-node cap for reads: the engine's node cap, `MAX_NODES` by default.
    fn max_expanded(&self) -> usize {
        crate::MAX_NODES
    }
}

/// A published revision. Keeping an old `Arc` never keeps the writer locked and never
/// exposes a staged mutation.
#[derive(Clone)]
pub struct ReadView {
    pub(super) source: Arc<dyn ViewSource>,
    /// Header and resource vectors (tokens, styles, assets, fonts); no nodes or notes.
    pub(super) snapshot: Arc<Snapshot>,
    pub(super) resource_bytes: OnceLock<usize>,
}

impl ReadView {
    /// Indexes a full snapshot (built once; the view never changes afterwards).
    pub fn new(mut snapshot: Snapshot) -> Self {
        let nodes = std::mem::take(&mut snapshot.nodes);
        let notes = std::mem::take(&mut snapshot.notes);
        let source = SnapshotSource::new(nodes, notes);
        Self::from_source(Arc::new(source), Arc::new(snapshot))
    }

    /// A view over an engine-owned source. `metadata` is the header with the resource
    /// vectors; its node and note vectors are ignored.
    pub fn from_source(source: Arc<dyn ViewSource>, metadata: Arc<Snapshot>) -> Self {
        Self {
            source,
            snapshot: metadata,
            resource_bytes: OnceLock::new(),
        }
    }

    /// Header and resource vectors only. Authored node/note access goes through the
    /// view; this is deliberately not a serialized/full snapshot representation.
    pub fn metadata(&self) -> &Arc<Snapshot> {
        &self.snapshot
    }

    pub fn source(&self) -> &Arc<dyn ViewSource> {
        &self.source
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.source.node(id)
    }

    pub fn note(&self, id: &str) -> Option<&Note> {
        self.source.note(id)
    }

    pub fn children(&self, id: &str) -> impl Iterator<Item = &str> {
        self.source.children(id).map(|n| n.id.as_str())
    }

    pub fn notes_on(&self, id: &str) -> impl Iterator<Item = &Note> {
        self.source.notes_on(id).into_iter()
    }

    pub fn len(&self) -> usize {
        self.source.node_count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn page(&self, id: &str) -> Option<&str> {
        self.source.page(id)
    }

    pub fn reference_users<'a>(&'a self, key: &str) -> impl Iterator<Item = &'a str> {
        self.source.reference_users(key).into_iter()
    }

    pub fn component_users(&self, master: &str) -> impl Iterator<Item = &str> {
        self.source.component_users(master).into_iter()
    }

    /// The `componentId` a node expands through (any kind, as readers follow it).
    pub(super) fn master(&self, id: &str) -> Option<&str> {
        self.node(id)?.typed_props().component_id()
    }

    /// Authored frames in source order `(parent_id, sort_key, id)`.
    pub(super) fn frames(&self) -> Vec<&Node> {
        let mut frames: Vec<_> = self
            .source
            .nodes()
            .filter(|n| n.kind == NodeKind::Frame)
            .collect();
        frames.sort_by(|a, b| {
            (&a.parent_id, a.sort_key, &a.id).cmp(&(&b.parent_id, b.sort_key, &b.id))
        });
        frames
    }

    /// Explicit full representation for APIs outside the scoped-reader path.
    /// Normal memory reads never call this compatibility conversion.
    pub fn to_snapshot(&self) -> Snapshot {
        let mut snapshot = self.snapshot.as_ref().clone();
        let mut nodes: Vec<&Node> = self.source.nodes().collect();
        nodes.sort_by(|a, b| {
            (&a.parent_id, a.sort_key, &a.id).cmp(&(&b.parent_id, b.sort_key, &b.id))
        });
        snapshot.nodes = nodes.into_iter().cloned().collect();
        snapshot.notes = self.source.notes().cloned().collect();
        snapshot
    }

    /// Trusted adapter boundary, equivalent to `ScopedSnapshot::from_validated`.
    /// The adapter guarantees the entire authored revision passed validation.
    pub fn scoped(&self) -> ScopedSnapshot<'_> {
        ScopedSnapshot::from_view(self)
    }

    /// Current retained view proxy, not a measured allocator/RSS result. An engine
    /// source shares most of its nodes with neighbouring revisions, so summing views
    /// overcounts.
    pub fn estimated_bytes(&self) -> usize {
        self.source.estimated_bytes()
            + *self
                .resource_bytes
                .get_or_init(|| resource_bytes(&self.snapshot))
    }

    pub(super) fn ordered<'a>(&'a self, ids: impl IntoIterator<Item = &'a str>) -> Vec<&'a Node> {
        let mut nodes: Vec<_> = ids.into_iter().filter_map(|id| self.node(id)).collect();
        nodes.sort_by(|a, b| {
            (&a.parent_id, a.sort_key, &a.id).cmp(&(&b.parent_id, b.sort_key, &b.id))
        });
        nodes
    }
}

pub(super) fn resource_bytes(snapshot: &Snapshot) -> usize {
    // Header plus resource vectors, as their serialized size.
    32 * 1024 + serde_json::to_vec(snapshot).map_or(0, |v| v.len())
}
