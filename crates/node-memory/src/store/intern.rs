//! Append-only string interner shared by every version of one project's state. Keys
//! are dense `u32`s and never reused, so a version can look up an id interned after
//! it was taken (it simply has no entry for that key). Interned names stay until the
//! project is evicted and reloaded.
use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};

#[derive(Default)]
struct Names {
    keys: HashMap<Arc<str>, u32>,
    names: Vec<Arc<str>>,
    bytes: usize,
}

#[derive(Default)]
pub(crate) struct Interner(RwLock<Names>);

impl Interner {
    fn read(&self) -> std::sync::RwLockReadGuard<'_, Names> {
        self.0.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// The key of `name`, if interned.
    pub(crate) fn get(&self, name: &str) -> Option<u32> {
        self.read().keys.get(name).copied()
    }

    /// The key of `name`, interning it first if needed.
    pub(crate) fn intern(&self, name: &str) -> u32 {
        if let Some(key) = self.get(name) {
            return key;
        }
        let mut names = self.0.write().unwrap_or_else(PoisonError::into_inner);
        if let Some(&key) = names.keys.get(name) {
            return key;
        }
        let key = u32::try_from(names.names.len()).expect("interned names fit u32");
        let name: Arc<str> = name.into();
        names.bytes += name.len() + 16 + 2 * std::mem::size_of::<Arc<str>>() + 12;
        names.names.push(name.clone());
        names.keys.insert(name, key);
        key
    }

    /// The name of an interned key.
    pub(crate) fn name(&self, key: u32) -> Arc<str> {
        self.read().names[key as usize].clone()
    }

    /// Bulk load: room for `additional` more names.
    pub(crate) fn reserve(&self, additional: usize) {
        let mut names = self.0.write().unwrap_or_else(PoisonError::into_inner);
        names.keys.reserve(additional);
        names.names.reserve(additional);
    }

    /// Estimated heap bytes: map slot, vector slot and the shared name.
    pub(crate) fn bytes(&self) -> usize {
        self.read().bytes
    }
}
