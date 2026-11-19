//! Latest-revision reads from the engine's published views: scoped reads and full
//! snapshots.
//!
//! A read is served from an immutable whole-project view only when memory holds the
//! database's current revision of the same incarnation, as the backend's identity
//! probe reports it. Otherwise the database snapshot serves the read and, when
//! memory was cold or behind, (re)loads it. Errors in this path never reach a read:
//! it falls back to the database path, which fails exactly as it does without memory.
use super::identity::Tag;
use super::stats::bump;
use super::{Loaded, Memory, ScopedRead};
use crate::{Result, SharedSnapshot, Snapshot, SnapshotCacheIdentity};
use fagent_scene_input::read_scope::ReadView;
use std::cmp::Ordering;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::AtomicU64;
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

/// What one latest-revision read runs on.
enum Latest<T> {
    Memory(Arc<ReadView>),
    Database(T),
}

/// A full snapshot with the identity of the read that produced it.
pub(crate) type Full = (Arc<Snapshot>, Option<SnapshotCacheIdentity>);

impl Memory {
    /// The snapshot a scoped read of `target` runs on: a published view when it is
    /// at the revision `probe` reports, else `load`, which then (re)loads memory.
    /// `load`'s errors are the database path's; a failed probe only skips memory.
    pub fn scoped_read(
        &self,
        target: &str,
        probe: impl FnOnce() -> Result<Option<SnapshotCacheIdentity>>,
        load: impl FnOnce() -> Result<Loaded>,
    ) -> Result<ScopedRead> {
        let counters = &self.inner.counters;
        let identity = probe().ok().flatten();
        let hits = (&counters.read_hits, &counters.read_fallbacks);
        let (latest, _) = self.latest(target, identity, load, |shared| &shared.snapshot, hits)?;
        Ok(match latest {
            Latest::Memory(view) => ScopedRead {
                shared: SharedSnapshot::memory(view),
                from_memory: true,
            },
            Latest::Database(shared) => ScopedRead::database(shared),
        })
    }

    /// The latest revision as a full snapshot, with the identity of the same read: a
    /// copy of the published view when memory holds the revision `probe` reports,
    /// else `load` (the database), which then (re)loads memory. Probe errors are the
    /// read's errors, as for a database read.
    pub fn full_read(
        &self,
        probe: impl FnOnce() -> Result<Option<SnapshotCacheIdentity>>,
        load: impl FnOnce() -> Result<Full>,
    ) -> Result<Full> {
        let counters = &self.inner.counters;
        let identity = probe()?;
        let hits = (&counters.full_hits, &counters.full_fallbacks);
        let (latest, identity) = self.latest("", identity, load, |s| s.as_ref(), hits)?;
        Ok(match latest {
            Latest::Memory(view) => (Arc::new(fagent_node_memory::full_snapshot(&view)), identity),
            Latest::Database(snapshot) => (snapshot, identity),
        })
    }

    /// A published view of `target` at `identity`, or `load`'s value, adopted into
    /// memory when memory was cold or behind. Returns the identity the result is at.
    fn latest<T>(
        &self,
        target: &str,
        identity: Option<SnapshotCacheIdentity>,
        load: impl FnOnce() -> Result<(T, Option<SnapshotCacheIdentity>)>,
        snapshot: impl FnOnce(&T) -> &Snapshot,
        (hits, fallbacks): (&AtomicU64, &AtomicU64),
    ) -> Result<(Latest<T>, Option<SnapshotCacheIdentity>)> {
        let counters = &self.inner.counters;
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
            bump(hits);
            return Ok((Latest::Memory(view), identity));
        }
        bump(fallbacks);
        let (value, loaded) = load()?;
        if let (Served::Cold | Served::Stale, Some(loaded)) = (&served, &loaded) {
            let reload = matches!(served, Served::Stale);
            let adopt = AssertUnwindSafe(|| self.adopt(snapshot(&value), loaded.clone(), reload));
            if catch_unwind(adopt).is_err() {
                bump(&counters.read_errors);
            }
        }
        Ok((Latest::Database(value), loaded))
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
    fn adopt(&self, snapshot: &Snapshot, identity: SnapshotCacheIdentity, reload: bool) {
        let (engine, tags, counters) = (&self.inner.engine, &self.inner.tags, &self.inner.counters);
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
        // The same or a newer revision of this incarnation may have been loaded or
        // committed since this read probed: keep it rather than replacing it with an
        // equal or older snapshot (a writer may be publishing into it).
        if let Some(current) = engine.project(&id) {
            if tags.tagged(&id, current.load_id(), &identity)
                && current.view().metadata().revision >= identity.revision
            {
                return;
            }
        }
        engine.evict(&id);
        let tag = match engine.load(snapshot.clone()) {
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
