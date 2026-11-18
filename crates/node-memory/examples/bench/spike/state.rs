//! One persistent project state for options (b) and (c): the node map, sorted children
//! and the live reference indexes a commit touches, all persistent, so the state is
//! both the writer's working copy and the readers' published view. A transaction
//! clones it in O(1), writes on the clone and is published or dropped whole.
use super::pmap::{Family, PMap};
use fagent_scene_input::Node;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, PoisonError, RwLock};

/// One node with its links and ancestor-derived indexes. `node` is shared with every
/// state version that did not rewrite it, so derived-only changes copy 32 bytes.
#[derive(Clone)]
pub struct Rec {
    pub node: Arc<Node>,
    pub parent: Option<u32>,
    pub page: u32,
    pub root: Option<u32>,
    pub master: Option<u32>,
}

impl PartialEq for Rec {
    /// Pointer equality on the node: the delta is found without comparing props.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.node, &other.node)
            && (self.parent, self.page, self.root, self.master)
                == (other.parent, other.page, other.root, other.master)
    }
}

pub type Set = Arc<BTreeSet<u32>>;

pub struct State<F: Family> {
    pub nodes: F::Map<Rec>,
    /// Children of a node in `(sort_key, id)` order; absent when it has none.
    pub children: F::Map<Arc<Vec<u32>>>,
    pub pages: Arc<Vec<u32>>,
    /// Instances by interned `componentId`.
    pub users: F::Map<Set>,
    /// Nodes by interned `links[].to`.
    pub links: F::Map<Set>,
    pub revision: u64,
    pub len: usize,
}

impl<F: Family> Clone for State<F> {
    fn clone(&self) -> Self {
        Self {
            nodes: self.nodes.clone(),
            children: self.children.clone(),
            pages: self.pages.clone(),
            users: self.users.clone(),
            links: self.links.clone(),
            revision: self.revision,
            len: self.len,
        }
    }
}

impl<F: Family> State<F> {
    pub fn rec(&self, key: u32) -> Option<&Rec> {
        self.nodes.get(key)
    }

    pub fn children(&self, parent: Option<u32>) -> &[u32] {
        match parent {
            Some(key) => self.children.get(key).map_or(&[], |list| list.as_slice()),
            None => &self.pages,
        }
    }

    pub fn subtree(&self, root: u32) -> Vec<u32> {
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(key) = stack.pop() {
            out.push(key);
            stack.extend(self.children(Some(key)).iter().rev());
        }
        out
    }
}

/// Append-only id interner shared by every state version. Ids of deleted nodes stay
/// interned until the project is reloaded.
#[derive(Default)]
pub struct Interner {
    ids: HashMap<Arc<str>, u32>,
    names: Vec<Arc<str>>,
}

impl Interner {
    pub fn get(&self, id: &str) -> Option<u32> {
        self.ids.get(id).copied()
    }

    pub fn intern(&mut self, id: &str) -> u32 {
        if let Some(key) = self.get(id) {
            return key;
        }
        let key = self.names.len() as u32;
        let name: Arc<str> = id.into();
        self.names.push(name.clone());
        self.ids.insert(name, key);
        key
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }
}

/// The engine as the spike models it: one writer at a time, readers clone the
/// published `Arc` and never wait on the writer.
pub struct Engine<F: Family> {
    pub interner: Interner,
    published: RwLock<Arc<State<F>>>,
}

impl<F: Family> Engine<F> {
    pub fn new(interner: Interner, state: State<F>) -> Self {
        Self {
            interner,
            published: RwLock::new(Arc::new(state)),
        }
    }

    pub fn view(&self) -> Arc<State<F>> {
        self.published
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Publishes a committed state: one pointer swap under the lock.
    pub fn publish(&self, next: State<F>) {
        let next = Arc::new(next);
        let mut guard = self
            .published
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        let old = std::mem::replace(&mut *guard, next);
        drop(guard);
        // Releasing the previous version (outside the lock) is part of the cost.
        drop(old);
    }
}
