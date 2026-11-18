//! A batch applied but not yet kept: the persistence layer writes its [`Delta`], then
//! commits, or drops it to keep the previous version. The batch lives on its own
//! version of the state; the project stays exclusively borrowed (or write-locked)
//! meanwhile, so no other writer races it and readers never see it before commit.
use super::ProjectState;
use crate::ops::Delta;
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
/// (also during a panic) discards it.
#[must_use = "a staged batch is discarded when dropped; call commit() to keep it"]
pub struct Staged<'a> {
    target: Target<'a>,
    next: ProjectState,
    delta: Delta,
    preview: Option<Arc<ReadView>>,
}

impl<'a> Staged<'a> {
    pub(crate) fn direct(state: &'a mut ProjectState, next: ProjectState, delta: Delta) -> Self {
        Self {
            target: Target::Direct(state),
            next,
            delta,
            preview: None,
        }
    }

    pub(crate) fn locked(
        guard: RwLockWriteGuard<'a, ProjectState>,
        bytes: &'a AtomicUsize,
        published: &'a RwLock<Arc<ReadView>>,
        next: ProjectState,
        delta: Delta,
    ) -> Self {
        Self {
            target: Target::Locked {
                guard,
                bytes,
                published,
            },
            next,
            delta,
            preview: None,
        }
    }

    /// The staged state (revision already incremented), for building the persisted
    /// rows or checking the result before committing.
    pub fn state(&self) -> &ProjectState {
        &self.next
    }

    /// The version the batch started from.
    pub fn base(&self) -> &ProjectState {
        match &self.target {
            Target::Direct(state) => state,
            Target::Locked { guard, .. } => guard,
        }
    }

    pub fn delta(&self) -> &Delta {
        &self.delta
    }

    /// The staged version as a reader view, never published until commit. Commit
    /// publishes this same `Arc`, so validation certificates bind to it.
    pub fn preview(&mut self) -> Arc<ReadView> {
        let next = &self.next;
        self.preview
            .get_or_insert_with(|| Arc::new(next.view()))
            .clone()
    }

    /// Keeps the batch and returns its delta.
    pub fn commit(mut self) -> Delta {
        let view = self.preview();
        let next = self.next;
        match self.target {
            Target::Direct(state) => *state = next,
            Target::Locked {
                mut guard,
                bytes,
                published,
            } => {
                bytes.store(next.estimated_bytes(), Ordering::Relaxed);
                *guard = next;
                let old = std::mem::replace(
                    &mut *published.write().unwrap_or_else(PoisonError::into_inner),
                    view,
                );
                // Release the previous version outside the publish lock.
                drop(guard);
                drop(old);
            }
        }
        self.delta
    }

    /// Discards the batch; the state is exactly as before `begin`.
    pub fn abort(self) {}
}
