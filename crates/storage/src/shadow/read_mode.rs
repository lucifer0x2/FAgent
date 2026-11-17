//! Task28 I2 switch and the snapshot type a scoped read runs on.
use crate::{SharedSnapshot, SnapshotCacheIdentity};

/// Environment switch: `1`/`on`/`true` serves scoped reads from memory (and turns
/// the shadow on); `strict` also asks callers to verify against today's path.
pub const NODE_MEMORY_READS_ENV: &str = "FAGENT_NODE_MEMORY_READS";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReadsMode {
    #[default]
    Off,
    On,
    Strict,
}

impl ReadsMode {
    pub fn from_env() -> Self {
        Self::parse(std::env::var(NODE_MEMORY_READS_ENV).ok().as_deref())
    }

    pub(super) fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("1" | "on" | "true") => Self::On,
            Some("strict") => Self::Strict,
            _ => Self::Off,
        }
    }
}

/// The snapshot one scoped read runs on.
pub struct ScopedRead {
    pub shared: SharedSnapshot,
    /// An immutable whole-project memory view rather than today's snapshot.
    pub from_memory: bool,
    /// Strict mode: the caller also runs the read on today's snapshot and compares.
    pub verify: bool,
}

impl ScopedRead {
    pub(crate) fn database(shared: SharedSnapshot) -> Self {
        Self {
            shared,
            from_memory: false,
            verify: false,
        }
    }
}

/// Today's path for one project: its shared snapshot and the identity of that read.
pub(crate) type Loaded = (SharedSnapshot, Option<SnapshotCacheIdentity>);
