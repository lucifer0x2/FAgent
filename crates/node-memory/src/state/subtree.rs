//! Allocation-light preorder walk returning borrowed nodes.
use crate::store::{Key, Store};
use fagent_scene_input::Node;

/// Preorder iterator over a subtree; holds one slice iterator per open level.
pub struct Subtree<'a> {
    store: &'a Store,
    first: Option<Key>,
    stack: Vec<std::slice::Iter<'a, Key>>,
}

impl<'a> Subtree<'a> {
    pub(crate) fn new(store: &'a Store, root: Key) -> Self {
        Self {
            store,
            first: Some(root),
            stack: Vec::new(),
        }
    }

    fn enter(&mut self, key: Key) -> &'a Node {
        let slot = self.store.slot(key);
        self.stack.push(slot.children.iter());
        &slot.node
    }
}

impl<'a> Iterator for Subtree<'a> {
    type Item = &'a Node;

    fn next(&mut self) -> Option<&'a Node> {
        if let Some(root) = self.first.take() {
            return Some(self.enter(root));
        }
        loop {
            match self.stack.last_mut()?.next() {
                Some(&key) => return Some(self.enter(key)),
                None => {
                    self.stack.pop();
                }
            }
        }
    }
}
