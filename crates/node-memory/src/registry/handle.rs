//! One cached project: many concurrent readers or one writer, plus access time and
//! size for the registry's eviction policy.
use super::Clock;
use crate::ops::Rejection;
use crate::{Delta, ProjectState, Staged};
use fagent_scene_input::read_scope::ReadView;
use fagent_scene_input::{Operation, Result};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard};
use std::time::Duration;

/// Identity of one load of a project into memory. Ids grow monotonically within a
/// registry, so two handles of the same project id are told apart by their load id
/// (an evicted and reloaded project gets a new one).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LoadId(pub(super) u64);

impl LoadId {
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Shared handle to a loaded project. Cloning the `Arc` never copies the project.
pub struct ProjectHandle {
    id: String,
    load_id: LoadId,
    state: RwLock<ProjectState>,
    published: RwLock<Arc<ReadView>>,
    clock: Arc<dyn Clock>,
    last_access_nanos: AtomicU64,
    bytes: AtomicUsize,
}

impl ProjectHandle {
    pub(super) fn new(state: ProjectState, clock: Arc<dyn Clock>, load_id: LoadId) -> Self {
        let view = Arc::new(state.view());
        let handle = Self {
            id: state.project_id().to_owned(),
            load_id,
            bytes: AtomicUsize::new(state.estimated_bytes()),
            state: RwLock::new(state),
            published: RwLock::new(view),
            last_access_nanos: AtomicU64::new(0),
            clock,
        };
        handle.touch();
        handle
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Which load of the project this handle is.
    pub fn load_id(&self) -> LoadId {
        self.load_id
    }

    /// Shared read access at one consistent revision. Writers wait while it is held.
    pub fn read(&self) -> RwLockReadGuard<'_, ProjectState> {
        self.touch();
        // A panicking batch already rolled back in `Txn`/`Staged` drop, so the state is
        // sound.
        self.state.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// Clone the last committed revision with no writer-state guard. Holding the
    /// returned Arc cannot delay commits or observe a batch before persistence.
    pub fn read_view(&self) -> Arc<ReadView> {
        self.touch();
        self.published
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Applies one atomic batch under the project's write lock and keeps it.
    pub fn apply(&self, ops: &[Operation]) -> Result<Delta> {
        Ok(self.begin(ops)?.commit())
    }

    /// Applies one atomic batch and holds it staged under the write lock: readers and
    /// other writers wait until the result is committed, aborted or dropped.
    pub fn begin(&self, ops: &[Operation]) -> Result<Staged<'_>> {
        self.touch();
        let state = self.state.write().unwrap_or_else(PoisonError::into_inner);
        let (next, delta, names) = state
            .stage(ops, Some(&self.bytes))
            .map_err(Rejection::into_error)?;
        Ok(Staged::locked(
            state,
            &self.bytes,
            &self.published,
            next,
            delta,
            names,
        ))
    }

    /// [`Self::begin`] when the project is at `expected` and the write lock is held:
    /// `None` when it is at another revision. Returns the published view the batch
    /// starts from together with the staging result, so a caller can inspect a
    /// rejected batch against the exact base without holding the lock.
    pub(crate) fn begin_at(
        &self,
        expected: u64,
        ops: &[Operation],
    ) -> Option<(Arc<ReadView>, std::result::Result<Staged<'_>, Rejection>)> {
        self.touch();
        let state = self.state.write().unwrap_or_else(PoisonError::into_inner);
        if state.revision() != expected {
            return None;
        }
        let before = self
            .published
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let staged = state
            .stage(ops, Some(&self.bytes))
            .map(|(next, delta, names)| {
                Staged::locked(state, &self.bytes, &self.published, next, delta, names)
            });
        Some((before, staged))
    }

    /// Estimated bytes as of the last load or successful batch, plus the names a
    /// discarded batch left interned (see `Staged`).
    pub fn estimated_bytes(&self) -> usize {
        self.bytes.load(Ordering::Relaxed)
    }

    /// Clock time of the last `get`, `read` or `apply`.
    pub fn last_access(&self) -> Duration {
        Duration::from_nanos(self.last_access_nanos.load(Ordering::Relaxed))
    }

    pub(super) fn touch(&self) {
        let now = self.clock.now().as_nanos() as u64;
        self.last_access_nanos.fetch_max(now, Ordering::Relaxed);
    }
}
