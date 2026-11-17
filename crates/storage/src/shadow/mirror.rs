//! The shadow registry: keeps each touched project HOT at the database's last
//! observed revision, stages every committed batch and keeps it only when it matches.
use super::compare::{self, Mismatch};
use super::stats::{add_micros, bump, COUNTERS};
use super::Commit;
use fagent_node_memory::{ProjectHandle, Registry, RegistryConfig, SystemClock};
use fagent_scene_input::Snapshot;
use serde_json::json;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::Duration;

/// Idle HOT projects are dropped after this, like the snapshot cache TTL.
const TTL: Duration = Duration::from_secs(60 * 60);
/// Soft cap on the estimated bytes of HOT projects.
const BUDGET_BYTES: usize = 2 << 30;
/// How often HOT read hits and writes reapply the TTL and budget.
const EXPIRE_EVERY: Duration = Duration::from_secs(60);

pub(super) enum Outcome {
    /// With the handle when this commit loaded the project, for incarnation tagging.
    Matched(Option<Arc<ProjectHandle>>),
    /// Memory was already past the commit's base revision (concurrent writers).
    Skipped,
}

pub(super) enum Failure {
    Mismatch(Mismatch),
    /// The shadow itself failed; nothing is known about the batch.
    Error(String),
}

impl From<Mismatch> for Failure {
    fn from(mismatch: Mismatch) -> Self {
        Self::Mismatch(mismatch)
    }
}

/// Where memory stands relative to a commit's base revision.
enum Found {
    /// The handle, and whether this commit loaded it.
    Ready(Arc<ProjectHandle>, bool),
    /// Already past it (concurrent writers).
    Past,
    /// Another loader (a scoped read) inserted the project first.
    Raced,
}

pub(super) struct Mirror {
    /// Shared with scoped reads (Task28 I2), which also load projects into it.
    pub(super) registry: Registry,
    strict: bool,
    /// Unix seconds of the last `expire`.
    last_expire: AtomicU64,
}

impl Mirror {
    pub fn new(strict: bool) -> Self {
        let config = RegistryConfig {
            ttl: TTL,
            budget_bytes: BUDGET_BYTES,
        };
        Self {
            registry: Registry::new(config, Arc::new(SystemClock::new())),
            strict,
            last_expire: AtomicU64::new(now_secs()),
        }
    }

    pub fn run(&self, c: &Commit<'_>) -> Result<Outcome, Failure> {
        // A read may load the project between our lookup and load: look once more.
        let found = match self.find(c)? {
            Found::Raced => self.find(c)?,
            found => found,
        };
        let (handle, loaded) = match found {
            Found::Ready(handle, loaded) => (handle, loaded),
            Found::Past => return Ok(Outcome::Skipped),
            Found::Raced => return Err(Failure::Error("load:PROJECT_EXISTS".into())),
        };
        let staged = handle.begin(c.operations).map_err(|e| {
            let at = e.op_index.map(|i| format!("@{i}")).unwrap_or_default();
            Mismatch::new(format!("rejected:{}{at}", e.code), [])
        })?;
        if staged.delta().revision != c.result.committed_revision {
            // Another observer moved the project between the check and the lock.
            return Ok(Outcome::Skipped);
        }
        compare::delta(staged.state(), staged.delta(), c)?;
        if self.strict {
            compare::full(staged.state(), c)?;
        }
        staged.commit();
        Ok(Outcome::Matched(loaded.then_some(handle)))
    }

    /// The project at the commit's base revision, loading it when cold or behind.
    fn find(&self, c: &Commit<'_>) -> Result<Found, Failure> {
        let id = c.before.project_id.as_str();
        let Some(handle) = self.registry.get(id) else {
            return self.load(c);
        };
        let revision = handle.read().revision();
        match revision.cmp(&c.before.revision) {
            Ordering::Equal if readiness_drifted(&handle, c) => {
                bump(&COUNTERS.resyncs);
                self.registry.remove(id);
                self.load(c)
            }
            Ordering::Equal => Ok(Found::Ready(handle, false)),
            Ordering::Greater => Ok(Found::Past),
            Ordering::Less => {
                bump(&COUNTERS.resyncs);
                self.registry.remove(id);
                self.load(c)
            }
        }
    }

    fn load(&self, c: &Commit<'_>) -> Result<Found, Failure> {
        let start = std::time::Instant::now();
        let mut before = c.before.clone();
        if let Some(ready) = c.ready {
            ready(&mut before);
        }
        match self.registry.load(before) {
            Ok((handle, _)) => {
                bump(&COUNTERS.loads);
                add_micros(&COUNTERS.load_micros, start);
                Ok(Found::Ready(handle, true))
            }
            Err(e) if e.code == "PROJECT_EXISTS" => Ok(Found::Raced),
            Err(e) => Err(Failure::Error(format!("load:{}", e.code))),
        }
    }

    /// Counts and logs a failure, then evicts the project; strict mode panics.
    pub fn fail(&self, c: &Commit<'_>, failure: Failure) {
        let project = &c.before.project_id;
        self.registry.remove(project);
        let (outcome, reason, ids) = match failure {
            Failure::Mismatch(m) => {
                bump(&COUNTERS.mismatches);
                ("mismatch", m.reason, m.ids)
            }
            Failure::Error(reason) => {
                bump(&COUNTERS.errors);
                ("error", reason, vec![])
            }
        };
        let line = json!({
            "event": "node_memory_shadow",
            "outcome": outcome,
            "project": project,
            "revision": c.result.committed_revision,
            "reason": reason,
            "ids": ids,
        });
        eprintln!("{line}");
        if self.strict {
            panic!("node-memory shadow {outcome}: {line}");
        }
    }

    pub fn evict(&self, project_id: &str) -> bool {
        self.registry.remove(project_id).is_some()
    }

    /// Applies TTL and the memory budget.
    pub fn expire(&self) {
        self.last_expire.store(now_secs(), AtomicOrdering::Relaxed);
        self.registry.evict();
    }

    /// `expire` at most once per `EXPIRE_EVERY`, for HOT read hits and writes, which
    /// otherwise never apply the TTL or recheck the budget after growth.
    pub fn maybe_expire(&self) {
        let now = now_secs();
        let last = self.last_expire.load(AtomicOrdering::Relaxed);
        if now.saturating_sub(last) >= EXPIRE_EVERY.as_secs()
            && self
                .last_expire
                .compare_exchange(last, now, AtomicOrdering::Relaxed, AtomicOrdering::Relaxed)
                .is_ok()
        {
            self.registry.evict();
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Asset and font readiness comes from storage at commit time, not from batches: a
/// file removed or restored out of band leaves memory with the readiness it was
/// loaded with. Such a project is reloaded instead of reported as a mismatch.
fn readiness_drifted(handle: &ProjectHandle, c: &Commit<'_>) -> bool {
    let Some(ready) = c.ready else {
        return false;
    };
    let before = c.before;
    let mut current = Snapshot {
        schema_version: before.schema_version,
        project_id: before.project_id.clone(),
        slug: String::new(),
        name: String::new(),
        revision: before.revision,
        nodes: Vec::new(),
        tokens: Vec::new(),
        styles: Vec::new(),
        assets: before.assets.clone(),
        fonts: before.fonts.clone(),
        notes: Vec::new(),
        retention: before.retention.clone(),
    };
    ready(&mut current);
    let view = handle.read_view();
    statuses(view.metadata()) != statuses(&current)
}

fn statuses(s: &Snapshot) -> BTreeMap<(&'static str, &str), &str> {
    let assets = s
        .assets
        .iter()
        .map(|a| (("asset", a.id.as_str()), a.status.as_str()));
    let fonts = s
        .fonts
        .iter()
        .map(|f| (("font", f.id.as_str()), f.status.as_str()));
    assets.chain(fonts).collect()
}

#[cfg(test)]
impl Mirror {
    pub fn hot_revision(&self, project_id: &str) -> Option<u64> {
        let handle = self.registry.get(project_id)?;
        let revision = handle.read().revision();
        Some(revision)
    }
}
