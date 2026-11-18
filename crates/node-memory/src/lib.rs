//! In-memory node engine: whole projects held in RAM with structural indexes
//! (children, parent, page, root frame, component master, instance usage, incoming
//! links, notes by anchor) that each write updates incrementally, in time proportional
//! to the affected subtree or sibling list.
//!
//! - [`Registry`] keeps HOT projects with TTL and memory-budget eviction.
//! - [`ProjectState`] answers borrowed reads, builds scoped [`ProjectState::slice`]s and
//!   applies atomic batches of stored [`Operation`]s with the same results as
//!   `Candidate::apply` (placement, merge rules, notes, link pruning), either at once
//!   or two-phase through [`Staged`], returning a [`Delta`] of changed rows.
//! - Each node records which revision last wrote each top-level props key and its
//!   placement since load ([`ProjectState::writes`]), for last-writer-wins reporting.
//!
//! [`ProjectHandle::read_view`] clones a committed whole-project reader revision;
//! payloads/index shards are shared until touched, and retaining it never holds the
//! writer-state lock. Storage publishes only after its database commit.
//!
//! Only structural validation runs here; property, token and style validation stays
//! in `fagent-scene-input`.
mod checks;
mod engine;
mod limits;
mod ops;
mod props;
mod registry;
mod state;
mod store;

pub use engine::{HotProject, MemoryConfig, MemoryStats, NodeMemory};
pub use fagent_scene_input::Operation;
pub use limits::Limits;
pub use ops::{Delta, RowChanges};
pub use registry::{
    Clock, Eviction, LoadId, ManualClock, ProjectHandle, ProjectInfo, Registry, RegistryConfig,
    SystemClock,
};
pub use state::{IndexDump, ProjectState, Staged, Subtree, Written};
