//! `create_node`: checks in `Candidate::apply` order, an O(1) insert, then placement
//! among the new siblings.
use super::Txn;
use crate::checks;
use crate::props::check_object;
use crate::store::Stamps;
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
        check_object(props)?;
        let node = Node {
            id: id.to_owned(),
            parent_id: parent_id.map(str::to_owned),
            sort_key: 0,
            kind: kind.clone(),
            props: props.clone(),
        };
        checks::instance_target(self.store, &node)?;
        self.check_index(id, parent, index)?;
        let stamps = Stamps::created(self.rev);
        let key = self.store.insert(node, parent, stamps, &mut self.log);
        let derived = self.store.derive(key);
        self.store.init(key, derived);
        self.touch(id, false);
        self.place(key, parent, index)
    }
}
