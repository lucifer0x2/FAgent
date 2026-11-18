//! Scoped reads from the engine's published views.
//!
//! A read is served from an immutable whole-project view only when memory holds the
//! database's current revision of the same incarnation, as the backend's identity
//! probe reports it. Otherwise the database snapshot serves the read and, when
//! memory was cold or behind, (re)loads it. Errors in this path never reach a read:
//! it falls back to the database path, which fails exactly as it does without memory.
use super::identity::Tag;
use super::stats::bump;
use super::{Loaded, Memory, ScopedRead};
use crate::{Result, SharedSnapshot, SnapshotCacheIdentity};
use fagent_scene_input::read_scope::ReadView;
use std::cmp::Ordering;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

enum Served {
    Hit(Arc<ReadView>),
    /// Not in memory.
    Cold,
    /// Behind the database, of another incarnation, or untagged.
    Stale,
    /// Fresh enough to keep, but this read cannot use it (unknown target, ahead).
    Pass,
}

impl Memory {
    /// The snapshot a scoped read of `target` runs on: a published view when it is
    /// at the revision `probe` reports, else `load`, which then (re)loads memory.
    /// `load`'s errors are the database path's.
    pub fn scoped_read(
        &self,
        target: &str,
        probe: impl FnOnce() -> Result<Option<SnapshotCacheIdentity>>,
        load: impl FnOnce() -> Result<Loaded>,
    ) -> Result<ScopedRead> {
        let counters = &self.inner.counters;
        let identity = probe().ok().flatten();
        let served = match &identity {
            Some(identity) => catch_unwind(AssertUnwindSafe(|| self.serve(identity, target)))
                .unwrap_or_else(|_| {
                    bump(&counters.read_errors);
                    Served::Pass
                }),
            None => Served::Pass,
        };
        self.inner.engine.maybe_expire();
        if let Served::Hit(view) = served {
            bump(&counters.read_hits);
            return Ok(ScopedRead {
                shared: SharedSnapshot::memory(view),
                from_memory: true,
            });
        }
        bump(&counters.read_fallbacks);
        let (shared, loaded) = load()?;
        if let (Served::Cold | Served::Stale, Some(loaded)) = (&served, loaded) {
            let reload = matches!(served, Served::Stale);
            let adopt = AssertUnwindSafe(|| self.adopt(&shared, loaded, reload));
            if catch_unwind(adopt).is_err() {
                bump(&counters.read_errors);
            }
        }
        Ok(ScopedRead::database(shared))
    }

    fn serve(&self, identity: &SnapshotCacheIdentity, target: &str) -> Served {
        let id = &identity.project_id;
        let Some(project) = self.inner.engine.project(id) else {
            return Served::Cold;
        };
        if !self.inner.tags.tagged(id, project.load_id(), identity) {
            return Served::Stale;
        }
        let view = project.settled_view(identity.revision);
        match view.metadata().revision.cmp(&identity.revision) {
            Ordering::Less => Served::Stale,
            Ordering::Greater => Served::Pass,
            Ordering::Equal => {
                if target.is_empty() || authored(&view, target).is_some() {
                    Served::Hit(view)
                } else {
                    Served::Pass
                }
            }
        }
    }

    /// Loads the database snapshot into memory, tagged with the incarnation it came
    /// from. A failed load (for example stored rows the engine rejects) is remembered
    /// for that identity so later reads do not retry it.
    fn adopt(&self, shared: &SharedSnapshot, identity: SnapshotCacheIdentity, reload: bool) {
        let (engine, tags, counters) = (&self.inner.engine, &self.inner.tags, &self.inner.counters);
        let snapshot = &shared.snapshot;
        if identity.revision != snapshot.revision || identity.project_id != snapshot.project_id {
            return;
        }
        let id = identity.project_id.clone();
        if tags
            .lock()
            .get(&id)
            .is_some_and(|tag| tag.failed_at(&identity))
        {
            return;
        }
        // A newer revision of the same incarnation may have been loaded since this
        // read probed: keep it rather than replacing it with an older snapshot.
        if let Some(current) = engine.project(&id) {
            if tags.tagged(&id, current.load_id(), &identity)
                && current.view().metadata().revision > identity.revision
            {
                return;
            }
        }
        engine.evict(&id);
        let tag = match engine.load(snapshot.as_ref().clone()) {
            Ok(project) => {
                bump(if reload {
                    &counters.read_reloads
                } else {
                    &counters.read_loads
                });
                Tag::loaded(project.load_id(), &identity)
            }
            // Another read loaded it meanwhile and tags it.
            Err(e) if e.code == "PROJECT_EXISTS" => return,
            Err(_) => {
                bump(&counters.read_errors);
                Tag::failed(&identity)
            }
        };
        tags.insert(engine, id, tag);
    }
}

/// The authored node a target is read through: itself, or the outermost instance of
/// a virtual `instance/child` id.
fn authored<'a>(state: &ReadView, target: &'a str) -> Option<&'a str> {
    if state.node(target).is_some() {
        return Some(target);
    }
    let (head, _) = target.split_once('/')?;
    state.node(head).map(|_| head)
}

#[cfg(test)]
impl Memory {
    pub fn failed(&self, project: &str) -> bool {
        self.inner
            .tags
            .lock()
            .get(project)
            .is_some_and(|tag| tag.failed)
    }

    pub fn engine(&self) -> &fagent_node_memory::NodeMemory {
        &self.inner.engine
    }
}
