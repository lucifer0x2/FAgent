//! A batch applied but not yet kept: the persistence layer writes its [`Delta`], then
//! commits, or aborts to restore the previous state exactly. The project stays
//! exclusively borrowed (or write-locked) meanwhile, so readers never see staged state.
use super::ProjectState;
use crate::ops::Delta;
use crate::store::Log;
use fagent_scene_input::read_scope::ReadView;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, PoisonError, RwLock, RwLockWriteGuard};

enum Target<'a> {
    Direct(&'a mut ProjectState),
    Locked {
        guard: RwLockWriteGuard<'a, ProjectState>,
        bytes: &'a AtomicUsize,
        published: &'a RwLock<Arc<ReadView>>,
    },
}

/// A staged batch. [`Staged::commit`] keeps it; [`Staged::abort`] or dropping it
/// (also during a panic) rolls it back.
#[must_use = "a staged batch is rolled back when dropped; call commit() to keep it"]
pub struct Staged<'a> {
    target: Target<'a>,
    /// `Some` while the batch can still be rolled back.
    log: Option<Log>,
    delta: Delta,
    preview: Option<Arc<ReadView>>,
}

impl<'a> Staged<'a> {
    pub(crate) fn direct(state: &'a mut ProjectState, log: Log, delta: Delta) -> Self {
        Self {
            target: Target::Direct(state),
            log: Some(log),
            delta,
            preview: None,
        }
    }

    pub(crate) fn locked(
        guard: RwLockWriteGuard<'a, ProjectState>,
        bytes: &'a AtomicUsize,
        published: &'a RwLock<Arc<ReadView>>,
        log: Log,
        delta: Delta,
    ) -> Self {
        Self {
            target: Target::Locked {
                guard,
                bytes,
                published,
            },
            log: Some(log),
            delta,
            preview: None,
        }
    }

    /// The staged state (revision already incremented), for building the persisted
    /// rows or checking the result before committing.
    pub fn state(&self) -> &ProjectState {
        match &self.target {
            Target::Direct(state) => state,
            Target::Locked { guard, .. } => guard,
        }
    }

    pub fn delta(&self) -> &Delta {
        &self.delta
    }

    /// Immutable staged successor for validation, never published until commit.
    /// Reused by commit so validation certificates bind to the published Arc.
    pub fn preview(&mut self) -> Arc<ReadView> {
        if let Some(view) = &self.preview {
            return view.clone();
        }
        let view = match &self.target {
            Target::Locked { published, .. } => {
                let before = published
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone();
                Arc::new(self.state().publish(&before, &self.delta))
            }
            // Direct ProjectState has no retained committed reader revision.
            Target::Direct(state) => Arc::new(ReadView::new(state.to_snapshot())),
        };
        self.preview = Some(view.clone());
        view
    }

    /// Keeps the batch and returns its delta.
    pub fn commit(mut self) -> Delta {
        if let Target::Locked {
            guard,
            bytes,
            published,
        } = &self.target
        {
            let before = published
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            let after = self
                .preview
                .clone()
                .unwrap_or_else(|| Arc::new(guard.publish(&before, &self.delta)));
            bytes.store(
                guard.estimated_bytes() + after.estimated_bytes(),
                Ordering::Relaxed,
            );
            *published.write().unwrap_or_else(PoisonError::into_inner) = after;
        }
        self.log = None;
        std::mem::take(&mut self.delta)
    }

    /// Rolls the batch back; the state is exactly as before `begin`.
    pub fn abort(self) {}
}

impl Drop for Staged<'_> {
    fn drop(&mut self) {
        let Some(log) = self.log.take() else {
            return;
        };
        match &mut self.target {
            Target::Direct(state) => state.unstage(log),
            Target::Locked {
                guard,
                bytes,
                published,
            } => {
                guard.unstage(log);
                // Admission can certify the retained before view even when this
                // candidate is rejected. Keep the registry budget proxy current.
                let view = published.read().unwrap_or_else(PoisonError::into_inner);
                bytes.store(
                    guard.estimated_bytes() + view.estimated_bytes(),
                    Ordering::Relaxed,
                );
            }
        }
    }
}
