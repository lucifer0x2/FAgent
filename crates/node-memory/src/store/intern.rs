//! Append-only string interner shared by every version of one project's state. Keys
//! are dense `u32`s, so a version can look up an id interned after it was taken (it
//! simply has no entry for that key). Interned names stay until the project is
//! evicted and reloaded, except the names a discarded batch added: [`Interner::close`]
//! removes them again when no other batch staged meanwhile and the caller knows that
//! no version holding them is alive, so a removed key is never visible to anyone.
use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};

#[derive(Default)]
struct Names {
    keys: HashMap<Arc<str>, u32>,
    names: Vec<Arc<str>>,
    bytes: usize,
    /// Batches opened so far, and how many are open now.
    opened: u64,
    open: u32,
}

/// The interner as one batch opened it.
pub(crate) struct Mark {
    len: usize,
    bytes: usize,
    opened: u64,
    /// No other batch was open when this one opened.
    alone: bool,
}

#[derive(Default)]
pub(crate) struct Interner(RwLock<Names>);

/// Estimated heap bytes of one name: map slot, vector slot and the shared string.
fn cost(name: &str) -> usize {
    name.len() + 16 + 2 * std::mem::size_of::<Arc<str>>() + 12
}

impl Interner {
    fn read(&self) -> std::sync::RwLockReadGuard<'_, Names> {
        self.0.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Names> {
        self.0.write().unwrap_or_else(PoisonError::into_inner)
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
        let mut names = self.write();
        if let Some(&key) = names.keys.get(name) {
            return key;
        }
        let key = u32::try_from(names.names.len()).expect("interned names fit u32");
        let name: Arc<str> = name.into();
        names.bytes += cost(&name);
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
        let mut names = self.write();
        names.keys.reserve(additional);
        names.names.reserve(additional);
    }

    /// Estimated heap bytes of every name.
    pub(crate) fn bytes(&self) -> usize {
        self.read().bytes
    }

    /// Opens a batch that may intern names; every mark is closed exactly once.
    pub(crate) fn open(&self) -> Mark {
        let mut names = self.write();
        names.opened += 1;
        let alone = names.open == 0;
        names.open += 1;
        Mark {
            len: names.names.len(),
            bytes: names.bytes,
            opened: names.opened,
            alone,
        }
    }

    /// Closes a batch. With `discard`, the caller guarantees that no live version
    /// holds a key the batch interned; the names are then removed when they are
    /// provably the batch's own: no other batch was open at any point in between.
    /// Returns the bytes of the names added since the mark that stay. When batches
    /// overlapped, that window also holds names the other batches interned, so the
    /// sum over-counts until the owner recomputes its estimate at the next commit:
    /// the safe direction for the byte budget (earlier eviction, never a missed one).
    /// Telling the names apart would need per-batch ownership on every intern.
    pub(crate) fn close(&self, mark: Mark, discard: bool) -> usize {
        let mut names = self.write();
        names.open -= 1;
        if discard && mark.alone && names.opened == mark.opened {
            let gone: Vec<Arc<str>> = names.names.drain(mark.len..).collect();
            for name in &gone {
                names.keys.remove(name);
            }
            names.bytes -= gone.iter().map(|n| cost(n)).sum::<usize>();
            debug_assert_eq!(names.bytes, mark.bytes);
            return 0;
        }
        names.bytes.saturating_sub(mark.bytes)
    }
}

/// The names one staged batch may add to a store's interners (node ids and reference
/// keys). Dropped without [`Interned::discard`], for example during a panic, it keeps
/// them.
pub(crate) struct Interned {
    ids: Arc<Interner>,
    ref_keys: Arc<Interner>,
    marks: Option<(Mark, Mark)>,
}

impl super::Store {
    /// Opens a batch on this store's interners, before the batch clones the store.
    pub(crate) fn open_names(&self) -> Interned {
        Interned {
            marks: Some((self.ids.open(), self.ref_keys.open())),
            ids: self.ids.clone(),
            ref_keys: self.ref_keys.clone(),
        }
    }
}

impl Interned {
    fn close(&mut self, discard: bool) -> usize {
        let Some((ids, ref_keys)) = self.marks.take() else {
            return 0;
        };
        self.ids.close(ids, discard) + self.ref_keys.close(ref_keys, discard)
    }

    /// The batch was kept: its names stay.
    pub(crate) fn keep(mut self) {
        self.close(false);
    }

    /// The batch was discarded. `unreachable`: no version that may hold the batch's
    /// keys is alive any more (the staged version, its views and clones are gone).
    /// Returns the bytes of names that stay interned.
    pub(crate) fn discard(mut self, unreachable: bool) -> usize {
        self.close(unreachable)
    }
}

impl Drop for Interned {
    fn drop(&mut self) {
        self.close(false);
    }
}
