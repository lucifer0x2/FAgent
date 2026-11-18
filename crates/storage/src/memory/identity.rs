//! The database incarnation each resident load came from. The engine stores no
//! identity; this map is the adapter's freshness record.
use crate::{SnapshotCacheIdentity, StorageError};
use fagent_node_memory::{LoadId, NodeMemory};
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
    /// The error a write got when loading this identity, returned to later writes.
    pub error: Option<StorageError>,
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

    pub fn failed_with(identity: &SnapshotCacheIdentity, error: StorageError) -> Self {
        let mut tag = Self::failed(identity);
        tag.error = Some(error);
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
            error: None,
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
    #[cfg(test)]
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

    /// The error a write remembered for a failed load of exactly `identity`.
    pub fn failed_error(&self, identity: &SnapshotCacheIdentity) -> Option<StorageError> {
        self.lock()
            .get(&identity.project_id)
            .filter(|tag| tag.failed_at(identity))
            .and_then(|tag| tag.error.clone())
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

    /// Records `tag` for `project`, dropping tags of loads that are no longer
    /// resident (failed tags stay: they stop repeated loads of a bad identity).
    pub fn insert(&self, engine: &NodeMemory, project: String, tag: Tag) {
        let mut all = self.lock();
        all.retain(|project, tag| {
            tag.failed || tag.load_id.is_some() && engine.resident(project) == tag.load_id
        });
        all.insert(project, tag);
    }

    /// Moves the tag of `load` to `identity` after a commit this process published;
    /// keeps its cache cleanup claim. Without an identity the load is untagged.
    pub fn retag(&self, project: &str, load: LoadId, identity: Option<&SnapshotCacheIdentity>) {
        let mut all = self.lock();
        let Some(tag) = all.get_mut(project).filter(|tag| tag.load_id == Some(load)) else {
            return;
        };
        match identity {
            Some(identity) => {
                let cleaned = tag.cache_cleaned;
                *tag = Tag::loaded(load, identity);
                tag.cache_cleaned = cleaned;
            }
            None => {
                all.remove(project);
            }
        }
    }

    pub fn remove(&self, project: &str) {
        self.lock().remove(project);
    }
}
