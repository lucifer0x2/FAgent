//! The in-memory project engine: one struct that owns every resident project and knows
//! nothing about where a snapshot came from or where a batch is persisted.
//!
//! An adapter loads snapshots with [`NodeMemory::load`], reads through
//! [`HotProject::view`], and commits through [`HotProject::begin`] (see `batch`). The
//! engine reads time only through [`Clock`], never reads the environment and never
//! spawns tasks: expiry runs when the adapter calls [`NodeMemory::maybe_expire`].
mod admission;
mod config;
mod project;
mod stats;

use crate::{Clock, Eviction, Registry, RegistryConfig};
pub use config::MemoryConfig;
use fagent_scene_input::{read_scope::ReadView, Result, Snapshot};
pub use project::HotProject;
pub use stats::MemoryStats;
pub(crate) use stats::Stats;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Every resident project, with TTL and byte-budget eviction.
pub struct NodeMemory {
    registry: Registry,
    clock: Arc<dyn Clock>,
    config: MemoryConfig,
    stats: Arc<Stats>,
    /// `Clock` nanoseconds of the last expiry sweep.
    last_expire: AtomicU64,
}

impl NodeMemory {
    pub fn new(config: MemoryConfig, clock: Arc<dyn Clock>) -> Self {
        let registry_config = RegistryConfig {
            ttl: config.ttl,
            budget_bytes: config.budget_bytes,
        };
        Self {
            registry: Registry::with_limits(registry_config, clock.clone(), config.limits),
            last_expire: AtomicU64::new(nanos(&*clock)),
            clock,
            config,
            stats: Arc::default(),
        }
    }

    /// Indexes `snapshot` and makes the project resident (structural checks only),
    /// admits it (full validation once; invalid stored data is still served, see
    /// `admission`), then applies the eviction policy to the others. Fails with
    /// `PROJECT_EXISTS` when the id is already resident: evict it first to reload,
    /// and treat a lost race as "someone else loaded it".
    pub fn load(&self, snapshot: Snapshot) -> Result<HotProject> {
        let start = nanos(&*self.clock);
        match self.registry.load(snapshot) {
            Ok((handle, evicted)) => {
                let micros = nanos(&*self.clock).saturating_sub(start) / 1_000;
                self.stats.load_micros.fetch_add(micros, Ordering::Relaxed);
                Stats::bump(&self.stats.loads);
                self.admit(&handle);
                self.count(&evicted);
                Ok(HotProject::new(handle, self.stats.clone()))
            }
            Err(error) => {
                if error.code != "PROJECT_EXISTS" {
                    Stats::bump(&self.stats.load_failures);
                }
                Err(error)
            }
        }
    }

    /// The resident project, refreshing its access time; `None` when cold.
    pub fn project(&self, project_id: &str) -> Option<HotProject> {
        let handle = self.registry.get(project_id)?;
        Some(HotProject::new(handle, self.stats.clone()))
    }

    /// The resident load of a project without refreshing its access time.
    pub fn resident(&self, project_id: &str) -> Option<crate::LoadId> {
        Some(self.registry.peek(project_id)?.load_id())
    }

    /// Drops a project. Holders of its [`HotProject`] keep it until they drop it.
    pub fn evict(&self, project_id: &str) -> bool {
        let removed = self.registry.remove(project_id).is_some();
        if removed {
            Stats::bump(&self.stats.evictions);
        }
        removed
    }

    /// Applies the TTL, then the byte budget.
    pub fn expire(&self) -> Vec<Eviction> {
        self.last_expire
            .store(nanos(&*self.clock), Ordering::Relaxed);
        let evicted = self.registry.evict();
        self.count(&evicted);
        evicted
    }

    /// [`Self::expire`] at most once per `expire_every`; empty when skipped. Call it on
    /// every read and commit: resident hits otherwise never apply the TTL or recheck
    /// the budget after growth.
    pub fn maybe_expire(&self) -> Vec<Eviction> {
        let now = nanos(&*self.clock);
        let last = self.last_expire.load(Ordering::Relaxed);
        let due = now.saturating_sub(last) >= self.config.expire_every.as_nanos() as u64;
        let claimed = due
            && self
                .last_expire
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok();
        if !claimed {
            return Vec::new();
        }
        let evicted = self.registry.evict();
        self.count(&evicted);
        evicted
    }

    /// A full copy of the latest committed revision; see [`full_snapshot`].
    pub fn to_snapshot(&self, project_id: &str) -> Option<Snapshot> {
        Some(full_snapshot(&self.project(project_id)?.view()))
    }

    pub fn stats(&self) -> MemoryStats {
        self.stats.snapshot()
    }

    /// Sum of estimated bytes of resident projects.
    pub fn total_bytes(&self) -> usize {
        self.registry.total_bytes()
    }

    fn count(&self, evicted: &[Eviction]) {
        for eviction in evicted {
            let counter = match eviction {
                Eviction::Expired(_) => &self.stats.expired,
                Eviction::OverBudget(_) => &self.stats.over_budget,
            };
            Stats::bump(counter);
        }
    }
}

/// A full copy of one published revision in the public entity order, as the
/// database adapters return it: nodes by parent, sort key and id; tokens, styles,
/// assets, fonts and notes by id (bytewise).
pub fn full_snapshot(view: &ReadView) -> Snapshot {
    let mut snapshot = view.to_snapshot();
    snapshot.tokens.sort_by(|a, b| a.id.cmp(&b.id));
    snapshot.styles.sort_by(|a, b| a.id.cmp(&b.id));
    snapshot.assets.sort_by(|a, b| a.id.cmp(&b.id));
    snapshot.fonts.sort_by(|a, b| a.id.cmp(&b.id));
    snapshot.notes.sort_by(|a, b| a.id.cmp(&b.id));
    snapshot
}

fn nanos(clock: &dyn Clock) -> u64 {
    clock.now().as_nanos() as u64
}
