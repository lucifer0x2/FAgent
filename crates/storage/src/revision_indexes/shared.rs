//! Explicit authority binding: identity metadata alone never grants read trust.
use crate::*;
use fagent_scene_input::read_scope::{ScopeIndex, ScopedSnapshot};

pub struct SharedSnapshot {
    pub snapshot: Arc<Snapshot>,
    pub indexes: Arc<RevisionIndexes>,
    authority: Option<(Arc<Snapshot>, Arc<RevisionIndexes>)>,
}

impl SharedSnapshot {
    /// Whole-project reader view. The metadata Snapshot intentionally contains
    /// resources/header only; scoped access uses the exact persistent payloads.
    pub(crate) fn memory(view: Arc<fagent_scene_input::read_scope::ReadView>) -> Self {
        let snapshot = view.metadata().clone();
        let indexes = Arc::new(RevisionIndexes {
            memory: Some(view),
            topology: Arc::default(),
            pages: Arc::default(),
            references: Default::default(),
            dependencies: Mutex::new(None),
            read_scope: Mutex::new(None),
            read_weight: 0,
        });
        Self::committed(snapshot, indexes)
    }
    /// Custom/embedded backends remain conservative, even with a cache identity.
    pub fn untrusted(snapshot: Arc<Snapshot>, indexes: Arc<RevisionIndexes>) -> Self {
        Self {
            snapshot,
            indexes,
            authority: None,
        }
    }

    /// Builtin storage owns the validated-write invariant. Bound to the exact
    /// immutable pair; callers replacing either public Arc lose this authority.
    pub(crate) fn committed(snapshot: Arc<Snapshot>, indexes: Arc<RevisionIndexes>) -> Self {
        Self {
            authority: Some((snapshot.clone(), indexes.clone())),
            snapshot,
            indexes,
        }
    }

    pub fn scoped(&self) -> Result<ScopedSnapshot<'_>> {
        if let Some(scoped) = self.trusted_scoped()? {
            return Ok(scoped);
        }
        ScopedSnapshot::validate(&self.snapshot)
    }

    /// Read layers with a legacy unvalidated fallback (for example find) can
    /// preserve it for custom backends rather than introducing new validation.
    pub fn trusted_scoped(&self) -> Result<Option<ScopedSnapshot<'_>>> {
        if !self.authority.as_ref().is_some_and(|(snapshot, indexes)| {
            Arc::ptr_eq(snapshot, &self.snapshot) && Arc::ptr_eq(indexes, &self.indexes)
        }) {
            return Ok(None);
        }
        if let Some(view) = &self.indexes.memory {
            return Ok(Arc::ptr_eq(view.metadata(), &self.snapshot).then(|| view.scoped()));
        }
        let Ok(mut cached) = self.indexes.read_scope.lock() else {
            // The slot only caches rebuildable metadata. A panicked holder must not
            // fail reads: callers keep their legacy path, as for custom backends.
            bench_phase::count("reads.scope_index_poisoned", 1);
            return Ok(None);
        };
        // Built on the first authoritative *snapshot vector*, not Candidate's
        // BTreeMap order.
        let index = match cached.as_ref() {
            Some(index) if index.indexes(&self.snapshot) => index.clone(),
            Some(_) => {
                // Same metadata, different vector: never dereference or replace
                // the other reader's index; build one for this snapshot only.
                bench_phase::count("reads.scope_index_mismatches", 1);
                Arc::new(ScopeIndex::new(&self.snapshot))
            }
            None => {
                bench_phase::count("reads.scope_index_builds", 1);
                let index = Arc::new(ScopeIndex::new(&self.snapshot));
                // Benchmark-only oversized note graphs are usable but not retained.
                if index.estimated_bytes() <= self.indexes.read_weight {
                    *cached = Some(index.clone());
                }
                index
            }
        };
        Ok(Some(ScopedSnapshot::from_validated(&self.snapshot, index)))
    }

    pub fn edit_context(&self, pages: Vec<String>) -> Result<Value> {
        if let Some(view) = &self.indexes.memory {
            let dependencies = view.dependency_closure(&pages)?;
            return Ok(crate::scoped::edit_context_response(
                view.metadata(),
                pages,
                &dependencies,
            ));
        }
        crate::scoped::edit_context_indexed(&self.snapshot, &self.indexes, pages)
    }
}
