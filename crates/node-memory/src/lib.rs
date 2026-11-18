//! In-memory node engine: whole projects held in RAM with structural indexes
//! (children, parent, page, root frame, component master, instance usage, incoming
//! links, notes by anchor) that each write updates incrementally, in time proportional
//! to the affected subtree or sibling list.
//!
//! - [`NodeMemory`] is the engine an adapter uses: it owns every resident project, loads
//!   snapshots, serves [`HotProject::view`]s and validates and stages batches through
//!   [`HotProject::begin`] (per-operation checks in `Txn`, then delta validation or the
//!   in-memory reference validator), returning the rows to persist.
//! - [`Registry`] keeps HOT projects with TTL and memory-budget eviction.
//! - [`ProjectState`] is one persistent value: nodes and indexes live in radix tries
//!   keyed by interned ids, so cloning it is O(1). It answers borrowed reads, builds
//!   scoped [`ProjectState::slice`]s and applies atomic batches of stored
//!   [`Operation`]s with the same results as `Candidate::apply` (placement, merge
//!   rules, notes, link pruning) on a clone, either at once or two-phase through
//!   [`Staged`], returning a [`Delta`] of changed rows. A rejected batch drops its
//!   clone; there is no undo log.
//! - Each node records which revision last wrote each top-level props key and its
//!   placement since load ([`ProjectState::writes`]), for last-writer-wins reporting.
//!
//! [`ProjectHandle::read_view`] returns the committed version itself as a reader
//! view (`ReadView` over [`ProjectState`]'s `ViewSource`): publishing is a pointer
//! swap, untouched nodes are shared between revisions, and retaining a view never
//! holds the writer-state lock. Storage publishes only after its database commit.
//!
//! `ProjectState` runs structural validation only; property, token and style validation
//! stays in `fagent-scene-input` and is driven by `HotProject::begin`.
mod batch;
mod checks;
mod engine;
mod limits;
mod ops;
mod props;
mod registry;
mod state;
mod store;

pub use batch::{Begin, Path, Prepared, Rows};
pub use engine::{HotProject, MemoryConfig, MemoryStats, NodeMemory};
pub use fagent_scene_input::Operation;
pub use limits::Limits;
pub use ops::{Delta, RowChanges};
pub use registry::{
    Clock, Eviction, LoadId, ManualClock, ProjectHandle, ProjectInfo, Registry, RegistryConfig,
    SystemClock,
};
pub use state::{IndexDump, ProjectState, Staged, Subtree, Written};
