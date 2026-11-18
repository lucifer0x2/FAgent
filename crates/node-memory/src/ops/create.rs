//! `create_node` in `Candidate::apply` order: id checks, the structural parent and
//! depth checks (the errors `check_node` would give first, made before the insert so
//! the store never links a bad parent), an O(1) insert, the node check
//! (`check_node`: properties, instance target, links) on the working state, then
//! placement, whose index check comes last as in `Candidate::place`.
use super::Txn;
use crate::checks;
use crate::store::Stamps;
use fagent_scene_input::validate::check_node;
use fagent_scene_input::{Node, NodeKind, Result, StorageError};
use serde_json::Value;

impl Txn<'_> {
    pub(super) fn create(
        &mut self,
        id: &str,
        kind: &NodeKind,
        parent_id: Option<&str>,
        props: &Value,
        index: Option<usize>,
    ) -> Result<()> {
        checks::not_virtual(id, parent_id)?;
        checks::valid(id)?;
        if self.store.contains(id) {
            return Err(fagent_scene_input::validate::structural::duplicate_node(id));
        }
        if self.store.res.notes.contains_key(id) {
            return Err(StorageError::validation(
                "id",
                "ID already belongs to a note",
            ));
        }
        let parent = checks::parent(self.store, id, kind, parent_id)?;
        checks::depth(1 + parent.map_or(0, |p| checks::depth_of(self.store, p)))?;
        let node = Node {
            id: id.to_owned(),
            parent_id: parent_id.map(str::to_owned),
            sort_key: 0,
            kind: kind.clone(),
            props: props.clone(),
        };
        let stamps = Stamps::created(self.rev);
        let key = self.store.insert(node, parent, stamps);
        check_node(&*self.store, self.store.node(key))?;
        let derived = self.store.derive(key);
        self.store.init(key, derived);
        self.touch(id, false);
        self.place(key, parent, index)
    }
}
