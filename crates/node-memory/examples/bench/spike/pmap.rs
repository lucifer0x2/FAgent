//! Persistent maps keyed by interned `u32` ids, behind one trait so the spike state is
//! the same code over `imbl::OrdMap`, `imbl::HashMap` and the hand-written radix trie.
//! `clone` is O(1) for all three: a transaction clones the state and writes on the copy.
use super::radix::Radix;

pub trait PMap<V: Clone>: Clone + Default {
    fn get(&self, key: u32) -> Option<&V>;
    fn insert(&mut self, key: u32, value: V);
    fn remove(&mut self, key: u32) -> Option<V>;
    /// Bulk build from entries in ascending key order.
    fn build(entries: Vec<(u32, V)>) -> Self;
    /// Keys whose value differs from `old` (added, removed or `!=`), skipping shared
    /// structure; `None` when the map cannot diff (the transaction's write set is used).
    fn diff(&self, old: &Self) -> Option<Vec<u32>>
    where
        V: PartialEq;
}

/// A family of maps: the spike state is generic over it.
pub trait Family {
    type Map<V: Clone>: PMap<V>;
    const NAME: &'static str;
}

pub struct Ord;
pub struct Hash;
pub struct Trie;

impl Family for Ord {
    type Map<V: Clone> = imbl::OrdMap<u32, V>;
    const NAME: &'static str = "imbl OrdMap";
}

impl Family for Hash {
    type Map<V: Clone> = imbl::HashMap<u32, V>;
    const NAME: &'static str = "imbl HashMap";
}

impl Family for Trie {
    type Map<V: Clone> = Radix<V>;
    const NAME: &'static str = "radix-64 trie";
}

impl<V: Clone> PMap<V> for imbl::OrdMap<u32, V> {
    fn get(&self, key: u32) -> Option<&V> {
        imbl::OrdMap::get(self, &key)
    }
    fn insert(&mut self, key: u32, value: V) {
        imbl::OrdMap::insert(self, key, value);
    }
    fn remove(&mut self, key: u32) -> Option<V> {
        imbl::OrdMap::remove(self, &key)
    }
    fn build(entries: Vec<(u32, V)>) -> Self {
        entries.into_iter().collect()
    }
    fn diff(&self, old: &Self) -> Option<Vec<u32>>
    where
        V: PartialEq,
    {
        use imbl::ordmap::DiffItem;
        let keys = old.diff(self).map(|item| match item {
            DiffItem::Add(k, _) | DiffItem::Remove(k, _) => *k,
            DiffItem::Update { new: (k, _), .. } => *k,
        });
        Some(keys.collect())
    }
}

impl<V: Clone> PMap<V> for imbl::HashMap<u32, V> {
    fn get(&self, key: u32) -> Option<&V> {
        imbl::HashMap::get(self, &key)
    }
    fn insert(&mut self, key: u32, value: V) {
        imbl::HashMap::insert(self, key, value);
    }
    fn remove(&mut self, key: u32) -> Option<V> {
        imbl::HashMap::remove(self, &key)
    }
    fn build(entries: Vec<(u32, V)>) -> Self {
        entries.into_iter().collect()
    }
    fn diff(&self, _old: &Self) -> Option<Vec<u32>>
    where
        V: PartialEq,
    {
        None
    }
}

impl<V: Clone> PMap<V> for Radix<V> {
    fn get(&self, key: u32) -> Option<&V> {
        Radix::get(self, key)
    }
    fn insert(&mut self, key: u32, value: V) {
        Radix::insert(self, key, value);
    }
    fn remove(&mut self, key: u32) -> Option<V> {
        Radix::remove(self, key)
    }
    fn build(entries: Vec<(u32, V)>) -> Self {
        let mut map = Radix::default();
        for (key, value) in entries {
            map.insert(key, value);
        }
        map
    }
    fn diff(&self, old: &Self) -> Option<Vec<u32>>
    where
        V: PartialEq,
    {
        Some(Radix::diff(self, old))
    }
}
