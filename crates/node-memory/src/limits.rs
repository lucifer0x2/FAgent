//! Per-project limits applied on load and by writes.
use fagent_scene_input::MAX_NODES;

/// Limits for one [`crate::ProjectState`]. The default matches the scene-input caps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum authored nodes, inclusive. Loads above it fail; creates stop at it.
    pub max_nodes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_nodes: MAX_NODES,
        }
    }
}
