//! Bounded revision metadata, independent from the typed snapshot admission cap.
use crate::{
    scoped::{dependency_index::DependencyIndex, index::PageIndex},
    *,
};
use moka::sync::Cache;

const DEPENDENCY_BUDGET: usize = 4 * 1024 * 1024;
// Default MAX_NOTES=5000, anchors <=128 bytes: <=1,160,000 proxy bytes.
const READ_NOTES_BUDGET: usize = 2 * 1024 * 1024;
const CACHE_BUDGET: u64 = 256 * 1024 * 1024;

pub struct RevisionIndexes {
    pub(crate) memory: Option<Arc<fagent_scene_input::read_scope::ReadView>>,
    pub(crate) topology: Arc<validate::NodeTopology>,
    pub(crate) pages: Arc<PageIndex>,
    /// Inverse authored reference usage, including nested overrides.
    pub(crate) references: BTreeMap<String, BTreeSet<String>>,
    dependencies: Mutex<Option<Arc<DependencyIndex>>>,
    pub(crate) read_scope: Mutex<Option<Arc<fagent_scene_input::read_scope::ScopeIndex>>>,
    pub(crate) read_weight: usize,
}

impl RevisionIndexes {
    pub(crate) fn new<'a>(nodes: impl IntoIterator<Item = &'a Node>) -> Self {
        bench_phase::count("indexes.topology_builds", 1);
        let nodes: Vec<_> = nodes.into_iter().collect();
        let topology = Arc::new(validate::NodeTopology::new(nodes.iter().copied()));
        let pages = Arc::new(PageIndex::with_topology(
            nodes.iter().copied(),
            topology.clone(),
        ));
        Self::with_tree(nodes, topology, pages)
    }

    fn with_tree<'a>(
        nodes: impl IntoIterator<Item = &'a Node>,
        topology: Arc<validate::NodeTopology>,
        pages: Arc<PageIndex>,
    ) -> Self {
        let mut references = BTreeMap::new();
        let mut read_weight = READ_NOTES_BUDGET;
        for node in nodes {
            scan(&node.props, &node.id, &mut references);
            // Reserve lazy read metadata before admission. Child-map overhead is
            // conservatively charged once per child, not once per unique parent.
            read_weight += 72 + node.id.len();
            if let Some(parent) = &node.parent_id {
                read_weight += 104 + parent.len();
            }
            for link in fagent_scene_input::node_links(&node.props) {
                if let Some(target) = link.to {
                    read_weight += 104 + target.len();
                }
            }
        }
        Self {
            memory: None,
            topology,
            pages,
            references,
            dependencies: Mutex::new(None),
            read_scope: Mutex::new(None),
            read_weight,
        }
    }

    pub(crate) fn candidate(
        before: &Arc<Self>,
        candidate: &Candidate,
        operations: &[Operation],
        changed: &BTreeSet<String>,
    ) -> Arc<Self> {
        if validate::NodeTopology::preserves(operations, changed) {
            bench_phase::count("indexes.topology_reused", 1);
            Arc::new(Self::with_tree(
                candidate.nodes.values(),
                before.topology.clone(),
                before.pages.clone(),
            ))
        } else {
            Arc::new(Self::new(candidate.nodes.values()))
        }
    }

    pub fn page(&self, node: &str) -> Option<&str> {
        if let Some(view) = &self.memory {
            return view.page(node);
        }
        self.pages.page(node)
    }
    pub fn component_users(&self, master: &str) -> Box<dyn Iterator<Item = &str> + '_> {
        if let Some(view) = &self.memory {
            return Box::new(view.component_users(master));
        }
        Box::new(self.topology.component_users(master))
    }
    pub fn reference_users(&self, key: &str) -> Box<dyn Iterator<Item = &str> + '_> {
        if let Some(view) = &self.memory {
            return Box::new(view.reference_users(key));
        }
        Box::new(
            self.references
                .get(key)
                .into_iter()
                .flatten()
                .map(String::as_str),
        )
    }

    pub(crate) fn dependencies(
        &self,
        build: impl FnOnce() -> Result<DependencyIndex>,
    ) -> Result<Arc<DependencyIndex>> {
        let mut cached = self.dependencies.lock().map_err(|_| poisoned())?;
        if let Some(index) = cached.as_ref() {
            return Ok(index.clone());
        }
        let index = Arc::new(build()?);
        bench_phase::count("indexes.dependency_builds", 1);
        if index.estimated_bytes() <= DEPENDENCY_BUDGET {
            *cached = Some(index.clone());
        }
        Ok(index)
    }

    fn weight(&self) -> u32 {
        let strings = |values: &BTreeMap<String, BTreeSet<String>>| {
            values
                .iter()
                .map(|(k, v)| 96 + k.len() + v.iter().map(|s| 64 + s.len()).sum::<usize>())
                .sum::<usize>()
        };
        let pages = self
            .pages
            .node_pages
            .iter()
            .map(|(a, b)| 96 + a.len() + b.len())
            .sum::<usize>()
            + self
                .pages
                .page_nodes
                .values()
                .chain(self.pages.roots.values())
                .chain([&self.pages.order])
                .map(|ids| ids.iter().map(|s| 24 + s.len()).sum::<usize>() + 96)
                .sum::<usize>();
        let roots = self
            .pages
            .root_ids
            .iter()
            .map(|id| 64 + id.len())
            .sum::<usize>();
        (DEPENDENCY_BUDGET
            + self.read_weight
            + 1024
            + self.topology.estimated_bytes()
            + pages
            + roots
            + strings(&self.references))
        .min(u32::MAX as usize) as u32
    }
}

#[derive(Clone)]
pub(crate) struct IndexCache {
    entries: Cache<SnapshotCacheIdentity, Arc<RevisionIndexes>>,
    gate: Arc<Mutex<()>>,
}
impl Default for IndexCache {
    fn default() -> Self {
        Self::limits(Duration::from_secs(3600), CACHE_BUDGET)
    }
}
impl IndexCache {
    fn limits(ttl: Duration, capacity: u64) -> Self {
        Self {
            entries: Cache::builder()
                .support_invalidation_closures()
                .max_capacity(capacity)
                .weigher(|_: &SnapshotCacheIdentity, value: &Arc<RevisionIndexes>| value.weight())
                .time_to_live(ttl)
                .build(),
            gate: Default::default(),
        }
    }
    pub(crate) fn evict(&self, project: &str) {
        let _gate = self
            .gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let project = project.to_owned();
        // All revision identities are discarded; in-flight readers own their Arcs.
        let _ = self
            .entries
            .invalidate_entries_if(move |key, _| key.project_id == project);
    }
    pub(crate) fn for_snapshot(
        &self,
        snapshot: &Snapshot,
        identity: Option<SnapshotCacheIdentity>,
    ) -> Arc<RevisionIndexes> {
        let Some(identity) = identity else {
            return Arc::new(RevisionIndexes::new(&snapshot.nodes));
        };
        {
            let _gate = self
                .gate
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(indexes) = self.entries.get(&identity) {
                bench_phase::count("indexes.cache_hits", 1);
                return indexes;
            }
        }
        let indexes = Arc::new(RevisionIndexes::new(&snapshot.nodes));
        let _gate = self
            .gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.entries.get_with(identity, || indexes)
    }
    pub(crate) fn publish(&self, identity: SnapshotCacheIdentity, indexes: Arc<RevisionIndexes>) {
        let _gate = self
            .gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Exact immutable keys: late old fills cannot replace any newer revision.
        self.entries.get_with(identity, || indexes);
    }
}

/// The immutable snapshot and indexes from one authoritative read.
mod shared;
pub use shared::SharedSnapshot;

fn scan(value: &Value, node: &str, refs: &mut BTreeMap<String, BTreeSet<String>>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if let Some(id) = value.as_str() {
                    let kind = match key.as_str() {
                        "tokenId" | "aliasId" => Some("token"),
                        "styleId" => Some("style"),
                        "fontId" => Some("font"),
                        "assetId" => Some("asset"),
                        "componentId" => Some("component"),
                        "to" => Some("link"),
                        _ => None,
                    };
                    if let Some(kind) = kind {
                        refs.entry(format!("{kind}:{id}"))
                            .or_default()
                            .insert(node.into());
                    }
                }
                scan(value, node, refs);
            }
        }
        Value::Array(values) => {
            for value in values {
                scan(value, node, refs);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod scale_tests;
#[cfg(test)]
mod scope_tests;
#[cfg(test)]
mod sqlite_tests;
#[cfg(test)]
mod tests;
