//! Small persistent string map: updating a key copies only its shared shard.
use std::collections::{hash_map::DefaultHasher, BTreeMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

const SHARDS: usize = 256;

#[derive(Clone)]
pub(crate) struct Map<T> {
    shards: Arc<[Arc<BTreeMap<String, T>>; SHARDS]>,
    bytes: usize,
    len: usize,
}

pub(crate) trait Weight {
    fn weight(&self) -> usize;
}
impl Weight for usize {
    fn weight(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

impl Weight for String {
    fn weight(&self) -> usize {
        self.len()
    }
}
impl Weight for Arc<std::collections::BTreeSet<String>> {
    fn weight(&self) -> usize {
        self.iter().map(|s| 64 + s.len()).sum()
    }
}
impl Weight for super::view_indexes::Siblings {
    fn weight(&self) -> usize {
        self.iter().map(|(_, id)| 72 + id.len()).sum()
    }
}
impl Weight for Arc<crate::Node> {
    fn weight(&self) -> usize {
        super::view::node_bytes(self)
    }
}
impl Weight for Arc<crate::Note> {
    fn weight(&self) -> usize {
        super::view::note_bytes(self)
    }
}

impl<T> Default for Map<T> {
    fn default() -> Self {
        let empty = Arc::new(BTreeMap::new());
        Self {
            shards: Arc::new(std::array::from_fn(|_| empty.clone())),
            bytes: 0,
            len: 0,
        }
    }
}

fn shard(key: &str) -> usize {
    let mut hash = DefaultHasher::new();
    key.hash(&mut hash);
    hash.finish() as usize % SHARDS
}

impl<T> Map<T> {
    pub fn get(&self, key: &str) -> Option<&T> {
        self.shards[shard(key)].get(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &T)> {
        self.shards.iter().flat_map(|part| part.iter())
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn estimated_bytes(&self) -> usize {
        self.bytes + SHARDS * 64
    }
}

impl<T: Clone + Weight> Map<T> {
    pub fn insert(&mut self, key: String, value: T) {
        if let Some(old) = self.get(&key) {
            self.bytes -= old.weight();
        } else {
            self.bytes += 96 + key.len();
            self.len += 1;
        }
        self.bytes += value.weight();
        let part = &mut Arc::make_mut(&mut self.shards)[shard(&key)];
        Arc::make_mut(part).insert(key, value);
    }

    pub fn remove(&mut self, key: &str) {
        if let Some(old) = self.get(key) {
            self.bytes -= 96 + key.len() + old.weight();
            self.len -= 1;
            let part = &mut Arc::make_mut(&mut self.shards)[shard(key)];
            Arc::make_mut(part).remove(key);
        }
    }
}
