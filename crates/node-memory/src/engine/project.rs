//! A resident project as handed to adapters.
use super::stats::Stats;
use crate::{LoadId, ProjectHandle};
use fagent_scene_input::read_scope::ReadView;
use std::fmt;
use std::sync::Arc;

/// A resident project: an `Arc` handle plus the load it belongs to. Cloning is cheap.
/// A handle outlives eviction, so compare [`HotProject::load_id`] with
/// [`crate::NodeMemory::resident`] to learn whether it is still the resident load.
#[derive(Clone)]
pub struct HotProject {
    pub(crate) handle: Arc<ProjectHandle>,
    pub(crate) stats: Arc<Stats>,
}

impl HotProject {
    pub(crate) fn new(handle: Arc<ProjectHandle>, stats: Arc<Stats>) -> Self {
        Self { handle, stats }
    }

    pub fn id(&self) -> &str {
        self.handle.id()
    }

    pub fn load_id(&self) -> LoadId {
        self.handle.load_id()
    }

    /// The latest published revision. Never blocks on a writer and never shows a
    /// batch before its commit.
    pub fn view(&self) -> Arc<ReadView> {
        self.handle.read_view()
    }

    /// The published view, waiting once for an in-flight batch when the published
    /// revision is behind `wanted`. PostgreSQL releases the project row lock when its
    /// transaction commits, before the batch publishes; without the wait the next
    /// reader would see the previous revision.
    pub fn settled_view(&self, wanted: u64) -> Arc<ReadView> {
        let view = self.view();
        if view.metadata().revision >= wanted {
            return view;
        }
        // A staged batch holds the state write lock until it publishes or aborts.
        drop(self.handle.read());
        self.view()
    }

    /// Estimated bytes as of the last load or successful batch, plus the names a
    /// discarded batch left interned (see `Staged`).
    pub fn estimated_bytes(&self) -> usize {
        self.handle.estimated_bytes()
    }
}

impl fmt::Debug for HotProject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HotProject")
            .field("id", &self.id())
            .field("load_id", &self.load_id())
            .finish()
    }
}
