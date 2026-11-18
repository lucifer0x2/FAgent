//! Post-commit cache eviction. The snapshot cache serves the latest revision of a
//! project, resident in memory or not; a commit or import drops its entry and the
//! next full-snapshot read loads the new revision and fills it again.
use crate::SnapshotCache;

/// The project a write transaction committed, and the namespace it observed.
pub(super) struct Committed {
    project_id: String,
    namespace: String,
}

impl Committed {
    pub fn new(project_id: String, namespace: String) -> Self {
        Self {
            project_id,
            namespace,
        }
    }

    /// Call only after the transaction committed. Evicts from a cache bound to the
    /// namespace that the write transaction observed.
    pub fn evict(self, cache: Option<&SnapshotCache>) {
        if let Some(cache) = cache.filter(|cache| cache.namespace() == self.namespace) {
            cache.evict(&self.project_id);
        }
    }
}

#[cfg(test)]
mod tests;
