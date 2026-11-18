//! The database incarnation each resident load came from. The engine stores no
//! identity; this map is the adapter's freshness record.
use crate::SnapshotCacheIdentity;
use fagent_node_memory::LoadId;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// One project's tag: the load it describes and the database identity it came from.
pub(super) struct Tag {
    /// `None` when loading this identity failed.
    pub load_id: Option<LoadId>,
    pub schema: u32,
    pub incarnation: String,
    pub readiness: Vec<bool>,
    pub revision: u64,
    /// The engine rejected this identity's snapshot; reads skip loading it again.
    pub failed: bool,
    /// Duplicate cache entries are removed once per load.
    pub cache_cleaned: bool,
}

impl Tag {
    pub fn loaded(load_id: LoadId, identity: &SnapshotCacheIdentity) -> Self {
        Self::new(Some(load_id), identity)
    }

    pub fn failed(identity: &SnapshotCacheIdentity) -> Self {
        let mut tag = Self::new(None, identity);
        tag.failed = true;
        tag
    }

    fn new(load_id: Option<LoadId>, identity: &SnapshotCacheIdentity) -> Self {
        Self {
            load_id,
            schema: identity.schema_version,
            incarnation: identity.incarnation.clone(),
            readiness: identity.asset_readiness.clone(),
            revision: identity.revision,
            failed: false,
            cache_cleaned: false,
        }
    }

    /// Same schema, incarnation and asset readiness: the revision may differ.
    pub fn same_incarnation(&self, identity: &SnapshotCacheIdentity) -> bool {
        self.schema == identity.schema_version
            && self.incarnation == identity.incarnation
            && self.readiness == identity.asset_readiness
    }

    /// A failed load of exactly this identity.
    pub fn failed_at(&self, identity: &SnapshotCacheIdentity) -> bool {
        self.failed && self.revision == identity.revision && self.same_incarnation(identity)
    }
}

#[derive(Default)]
pub(super) struct Tags(Mutex<HashMap<String, Tag>>);

impl Tags {
    pub fn lock(&self) -> MutexGuard<'_, HashMap<String, Tag>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether `load` of `project` carries a tag of any incarnation.
    pub fn tags_load(&self, project: &str, load: LoadId) -> bool {
        self.lock()
            .get(project)
            .is_some_and(|tag| tag.load_id == Some(load))
    }

    /// Whether `load` of `project` is tagged with the incarnation of `identity`.
    pub fn tagged(&self, project: &str, load: LoadId, identity: &SnapshotCacheIdentity) -> bool {
        self.lock()
            .get(project)
            .is_some_and(|tag| tag.load_id == Some(load) && tag.same_incarnation(identity))
    }

    pub fn claim_cache_cleanup(&self, project: &str, load: LoadId) -> bool {
        let mut tags = self.lock();
        let Some(tag) = tags.get_mut(project) else {
            return false;
        };
        if tag.load_id != Some(load) || tag.cache_cleaned {
            return false;
        }
        tag.cache_cleaned = true;
        true
    }

    pub fn remove(&self, project: &str) {
        self.lock().remove(project);
    }
}
