//! Task28 I2: scoped reads from the in-memory project.
//!
//! A read is served from an immutable whole-project view only when memory holds the
//! database's current revision of the same incarnation, as the backend's cache
//! identity probe reports it. Otherwise today's snapshot serves the read and, when
//! memory was cold or behind, (re)loads it. Errors in this path never reach a read:
//! it falls back to today's path, which fails exactly as it does without memory.
//!
use super::mirror::Mirror;
use super::read_mode::{Loaded, ReadsMode, ScopedRead};
use super::stats::{bump, COUNTERS};
use crate::{Result, SharedSnapshot, SnapshotCacheIdentity};
use fagent_node_memory::ProjectHandle;
use fagent_scene_input::read_scope::ReadView;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex, PoisonError, Weak};

/// The database incarnation a HOT project was loaded from by a read.
struct Tag {
    handle: Weak<ProjectHandle>,
    identity: SnapshotCacheIdentity,
    /// The lib rejected this identity's snapshot; reads skip loading it again.
    failed: bool,
    /// Duplicate legacy cache entries are removed once per admitted handle.
    cache_cleaned: bool,
}

enum Served {
    Hit(Arc<ReadView>),
    /// Not in memory.
    Cold,
    /// Behind the database, of another incarnation, or loaded without one.
    Stale,
    /// Fresh enough to keep, but this read cannot use it (unknown target, ahead).
    Pass,
}

pub(super) struct Reads {
    verify: bool,
    tags: Mutex<HashMap<String, Tag>>,
}

impl Reads {
    pub fn new(mode: ReadsMode) -> Option<Self> {
        (mode != ReadsMode::Off).then(|| Self {
            verify: mode == ReadsMode::Strict,
            tags: Mutex::default(),
        })
    }

    pub fn read(
        &self,
        mirror: &Mirror,
        target: &str,
        probe: impl FnOnce() -> Result<Option<SnapshotCacheIdentity>>,
        load: impl FnOnce() -> Result<Loaded>,
    ) -> Result<ScopedRead> {
        let identity = probe().ok().flatten();
        let served = match &identity {
            Some(identity) => {
                catch_unwind(AssertUnwindSafe(|| self.serve(mirror, identity, target)))
                    .unwrap_or_else(|_| {
                        bump(&COUNTERS.read_errors);
                        Served::Pass
                    })
            }
            None => Served::Pass,
        };
        mirror.maybe_expire();
        if let Served::Hit(view) = served {
            bump(&COUNTERS.read_hits);
            return Ok(ScopedRead {
                shared: SharedSnapshot::memory(view),
                from_memory: true,
                verify: self.verify,
            });
        }
        bump(&COUNTERS.read_fallbacks);
        let (shared, loaded) = load()?;
        if let (Served::Cold | Served::Stale, Some(loaded)) = (&served, loaded) {
            let reload = matches!(served, Served::Stale);
            let adopt = AssertUnwindSafe(|| self.adopt(mirror, &shared, loaded, reload));
            if catch_unwind(adopt).is_err() {
                bump(&COUNTERS.read_errors);
            }
        }
        Ok(ScopedRead::database(shared))
    }

    fn serve(&self, mirror: &Mirror, identity: &SnapshotCacheIdentity, target: &str) -> Served {
        let Some(handle) = mirror.registry.get(&identity.project_id) else {
            return Served::Cold;
        };
        if !self.tagged(&handle, identity) {
            return Served::Stale;
        }
        published_revision(&handle, identity.revision);
        let view = handle.read_view();
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

    /// Loads today's snapshot into memory, tagged with the incarnation it came from.
    /// A failed load (for example stored rows the lib rejects) is remembered for that
    /// identity so later reads do not retry it.
    fn adopt(
        &self,
        mirror: &Mirror,
        shared: &SharedSnapshot,
        identity: SnapshotCacheIdentity,
        reload: bool,
    ) {
        let snapshot = &shared.snapshot;
        if identity.revision != snapshot.revision || identity.project_id != snapshot.project_id {
            return;
        }
        let id = identity.project_id.clone();
        if self
            .tags()
            .get(&id)
            .is_some_and(|t| t.failed && t.identity == identity)
        {
            return;
        }
        // A writer may have published a newer revision of the same incarnation since
        // this read probed: keep it rather than replacing it with an older snapshot.
        if let Some(current) = mirror.registry.get(&id) {
            let same_incarnation = self.tags().get(&id).is_some_and(|tag| {
                std::ptr::eq(tag.handle.as_ptr(), Arc::as_ptr(&current))
                    && tag.identity.schema_version == identity.schema_version
                    && tag.identity.incarnation == identity.incarnation
            });
            if same_incarnation && current.read_view().metadata().revision > identity.revision {
                return;
            }
        }
        mirror.registry.remove(&id);
        let (handle, failed) = match mirror.registry.load(snapshot.as_ref().clone()) {
            Ok((handle, _)) => {
                bump(if reload {
                    &COUNTERS.read_reloads
                } else {
                    &COUNTERS.read_loads
                });
                (Arc::downgrade(&handle), false)
            }
            // `PROJECT_EXISTS`: the mirror loaded it meanwhile and tags nothing.
            Err(e) if e.code == "PROJECT_EXISTS" => return,
            Err(_) => {
                bump(&COUNTERS.read_errors);
                (Weak::new(), true)
            }
        };
        let mut tags = self.tags();
        tags.retain(|_, tag| tag.failed || tag.handle.strong_count() > 0);
        let tag = Tag {
            handle,
            identity,
            failed,
            cache_cleaned: false,
        };
        tags.insert(id, tag);
        drop(tags);
        mirror.expire();
    }

    /// Tags a project the mirror or a write loaded with the incarnation it came from,
    /// so the first read does not reload it.
    pub fn tag(&self, handle: &Arc<ProjectHandle>, identity: SnapshotCacheIdentity) {
        let mut tags = self.tags();
        tags.retain(|_, tag| tag.failed || tag.handle.strong_count() > 0);
        let handle = Arc::downgrade(handle);
        let cache_cleaned = tags
            .get(&identity.project_id)
            .is_some_and(|tag| tag.handle.ptr_eq(&handle) && tag.cache_cleaned);
        let tag = Tag {
            handle,
            identity,
            failed: false,
            cache_cleaned,
        };
        tags.insert(tag.identity.project_id.clone(), tag);
    }

    /// Whether `handle` carries a tag of any incarnation.
    pub fn tags_handle(&self, handle: &Arc<ProjectHandle>) -> bool {
        self.tags()
            .get(handle.id())
            .is_some_and(|tag| std::ptr::eq(tag.handle.as_ptr(), Arc::as_ptr(handle)))
    }

    pub fn claim_cache_cleanup(&self, handle: &Arc<ProjectHandle>) -> bool {
        let mut tags = self.tags();
        let Some(tag) = tags.get_mut(handle.id()) else {
            return false;
        };
        if !std::ptr::eq(tag.handle.as_ptr(), Arc::as_ptr(handle)) || tag.cache_cleaned {
            return false;
        }
        tag.cache_cleaned = true;
        true
    }

    pub(super) fn tagged(
        &self,
        handle: &Arc<ProjectHandle>,
        identity: &SnapshotCacheIdentity,
    ) -> bool {
        self.tags().get(handle.id()).is_some_and(|tag| {
            std::ptr::eq(tag.handle.as_ptr(), Arc::as_ptr(handle))
                && tag.identity.schema_version == identity.schema_version
                && tag.identity.incarnation == identity.incarnation
                && tag.identity.asset_readiness == identity.asset_readiness
        })
    }

    fn tags(&self) -> std::sync::MutexGuard<'_, HashMap<String, Tag>> {
        self.tags.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The handle's published revision, waiting once for an in-flight batch when it is
/// behind `wanted`. PostgreSQL releases the project row lock when its transaction
/// commits, before the HOT batch publishes; without the wait the next writer or
/// reader sees the previous revision and takes the full path or reloads.
pub(super) fn published_revision(handle: &ProjectHandle, wanted: u64) -> u64 {
    let revision = handle.read_view().metadata().revision;
    if revision >= wanted {
        return revision;
    }
    // A staged batch holds the state write lock until it publishes or aborts.
    drop(handle.read());
    handle.read_view().metadata().revision
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
impl Reads {
    pub fn failed(&self, project_id: &str) -> bool {
        self.tags().get(project_id).is_some_and(|tag| tag.failed)
    }
}
