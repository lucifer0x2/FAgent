//! Build a reader revision from exact committed rows, never from a full snapshot.
use super::ProjectState;
use crate::Delta;
use fagent_scene_input::read_scope::ReadView;

impl ProjectState {
    pub(crate) fn publish(&self, before: &ReadView, delta: &Delta) -> ReadView {
        let nodes = delta
            .nodes
            .upserted
            .iter()
            .chain(&delta.nodes.deleted)
            .map(|id| (id.as_str(), self.node(id)));
        let notes = delta
            .notes
            .upserted
            .iter()
            .chain(&delta.notes.deleted)
            .map(|id| (id.as_str(), self.note(id)));
        let resources = (!delta.tokens.is_empty() || !delta.styles.is_empty()).then(|| {
            let mut meta = self.meta.clone();
            self.store.res.fill(&mut meta);
            meta.notes.clear();
            meta
        });
        before.advance(
            delta.revision,
            nodes,
            notes,
            resources.map(|meta| (meta, delta.tokens.as_slice(), delta.styles.as_slice())),
        )
    }
}
