//! Validation over the state itself: admission of a loaded revision (full validation
//! once, for the result) and the end-of-batch delta rules of every staged batch, with
//! the [`Facts`] they carry from one revision to the next.
use super::ProjectState;
use crate::ops::Delta;
use fagent_scene_input::validate::delta::{facts, validate_batch, Batch, Facts};
use fagent_scene_input::validate::Candidate;
use fagent_scene_input::{bench_phase::time, NodeKind, Result};
use std::sync::Arc;

impl ProjectState {
    /// Admission: validates the whole revision once (`Candidate::validate`) and
    /// computes its facts. Invalid stored data stays loaded and served; later batches
    /// are judged on their delta, so a stored defect matters only when a batch's
    /// closure reaches it. Returns the validation result.
    pub fn admit(&self) -> Result<()> {
        let result = time("memory.admission.validate", || {
            Candidate::from_snapshot(&self.to_snapshot()).validate()
        });
        self.facts();
        result
    }

    /// The facts of this revision, computed over every instance on first use (a
    /// state built without [`Self::admit`]).
    pub(super) fn facts(&self) -> &Arc<Facts> {
        self.facts.get_or_init(|| {
            time("memory.admission.facts", || {
                let instances = self.store.iter().map(|(_, slot)| &slot.node);
                let instances = instances.filter(|n| n.kind == NodeKind::Instance);
                Arc::new(facts(&self.store, instances.map(|n| n.id.as_str())))
            })
        })
    }

    /// The end-of-batch rules of the batch that turned `self` into `next` (with
    /// `delta`), on the delta and its closure only; stores the facts of `next`.
    pub(super) fn validate_batch(&self, next: &mut ProjectState, delta: &Delta) -> Result<()> {
        let batch = Batch {
            nodes: &delta.nodes.upserted,
            deleted: &delta.nodes.deleted,
            notes: &delta.notes.upserted,
            tokens: &delta.tokens,
            styles: &delta.styles,
            max_nodes: self.limits.max_nodes,
        };
        let facts = validate_batch(&self.store, &next.store, &batch, self.facts())?;
        next.facts = Arc::new(facts).into();
        Ok(())
    }
}
