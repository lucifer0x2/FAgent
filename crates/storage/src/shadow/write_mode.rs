//! Task28 I3 switch: commits through the in-memory project.

/// Environment switch: `1`/`on`/`true` commits batches through memory (and turns
/// memory reads and the mirror on); `strict` also checks every staged batch against
/// the validator's candidate and panics on a difference (tests only).
pub const NODE_MEMORY_WRITES_ENV: &str = "FAGENT_NODE_MEMORY_WRITES";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WritesMode {
    #[default]
    Off,
    On,
    Strict,
}

impl WritesMode {
    pub fn from_env() -> Self {
        Self::parse(std::env::var(NODE_MEMORY_WRITES_ENV).ok().as_deref())
    }

    pub(super) fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("1" | "on" | "true") => Self::On,
            Some("strict") => Self::Strict,
            _ => Self::Off,
        }
    }
}
