//! Tunables of one [`crate::NodeMemory`].
use crate::Limits;
use std::time::Duration;

/// Idle projects are dropped after this, like the snapshot cache TTL.
pub const DEFAULT_TTL: Duration = Duration::from_secs(60 * 60);
/// Soft cap on the estimated bytes of resident projects.
pub const DEFAULT_BUDGET_BYTES: usize = 2 << 30;
/// How often [`crate::NodeMemory::maybe_expire`] reapplies the TTL and budget.
pub const DEFAULT_EXPIRE_EVERY: Duration = Duration::from_secs(60);

/// Residency policy and per-project limits.
#[derive(Debug, Clone, Copy)]
pub struct MemoryConfig {
    /// A project idle for this long is dropped by `expire`.
    pub ttl: Duration,
    /// Soft cap on the sum of estimated project bytes; least recently used first.
    pub budget_bytes: usize,
    /// Minimum time between two `maybe_expire` sweeps.
    pub expire_every: Duration,
    pub limits: Limits,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            ttl: DEFAULT_TTL,
            budget_bytes: DEFAULT_BUDGET_BYTES,
            expire_every: DEFAULT_EXPIRE_EVERY,
            limits: Limits::default(),
        }
    }
}
