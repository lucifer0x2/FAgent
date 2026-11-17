//! Project registry: which projects are HOT (in memory). The map lock is held only to
//! look up, insert or drop an `Arc`, so work on one project never blocks another.
mod clock;
mod evict;
mod handle;

use crate::ProjectState;
use fagent_scene_input::{Result, Snapshot, StorageError};
use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};
use std::time::Duration;

pub use clock::{Clock, ManualClock, SystemClock};
pub use evict::Eviction;
pub use handle::ProjectHandle;

/// HOT/COLD policy: a project idle for `ttl` or beyond the memory budget (least
/// recently used first) becomes COLD, i.e. is dropped from the registry.
#[derive(Debug, Clone, Copy)]
pub struct RegistryConfig {
    pub ttl: Duration,
    /// Soft cap on the sum of estimated project bytes.
    pub budget_bytes: usize,
}

/// One row of [`Registry::list`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectInfo {
    pub project_id: String,
    pub estimated_bytes: usize,
    pub last_access: Duration,
}

pub struct Registry {
    projects: RwLock<HashMap<String, Arc<ProjectHandle>>>,
    config: RegistryConfig,
    clock: Arc<dyn Clock>,
}

impl Registry {
    pub fn new(config: RegistryConfig, clock: Arc<dyn Clock>) -> Self {
        Self {
            projects: RwLock::default(),
            config,
            clock,
        }
    }

    /// Indexes `snapshot` and makes it HOT, then evicts other projects as the policy
    /// requires (the new project is never evicted by its own load). Fails with
    /// `PROJECT_EXISTS` when the id is already loaded; remove it first to reload.
    pub fn load(&self, snapshot: Snapshot) -> Result<(Arc<ProjectHandle>, Vec<Eviction>)> {
        let id = snapshot.project_id.clone();
        if self.map().contains_key(&id) {
            return Err(exists());
        }
        let state = ProjectState::from_snapshot(snapshot)?;
        let handle = Arc::new(ProjectHandle::new(state, self.clock.clone()));
        {
            let mut map = self.map_mut();
            if map.contains_key(&id) {
                return Err(exists());
            }
            map.insert(id.clone(), handle.clone());
        }
        let evicted = self.evict_except(Some(&id));
        Ok((handle, evicted))
    }

    /// The HOT project, refreshing its access time; `None` when COLD.
    pub fn get(&self, project_id: &str) -> Option<Arc<ProjectHandle>> {
        let handle = self.map().get(project_id).cloned()?;
        handle.touch();
        Some(handle)
    }

    /// HOT projects ordered by id.
    pub fn list(&self) -> Vec<ProjectInfo> {
        let mut rows: Vec<_> = self
            .map()
            .values()
            .map(|h| ProjectInfo {
                project_id: h.id().to_owned(),
                estimated_bytes: h.estimated_bytes(),
                last_access: h.last_access(),
            })
            .collect();
        rows.sort_by(|a, b| a.project_id.cmp(&b.project_id));
        rows
    }

    /// Drops a project from memory. Holders of its handle keep it until they drop it.
    pub fn remove(&self, project_id: &str) -> Option<Arc<ProjectHandle>> {
        self.map_mut().remove(project_id)
    }

    /// Applies TTL, then the memory budget. Call periodically.
    pub fn evict(&self) -> Vec<Eviction> {
        self.evict_except(None)
    }

    /// Sum of estimated bytes of HOT projects.
    pub fn total_bytes(&self) -> usize {
        self.map().values().map(|h| h.estimated_bytes()).sum()
    }

    fn map(&self) -> std::sync::RwLockReadGuard<'_, HashMap<String, Arc<ProjectHandle>>> {
        self.projects.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn map_mut(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<String, Arc<ProjectHandle>>> {
        self.projects
            .write()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

fn exists() -> StorageError {
    StorageError::new("PROJECT_EXISTS", "Project is already loaded")
}
