//! Last-writer-wins groundwork: per node, the revision that last wrote each property
//! since the project was loaded. A property is a top-level props key, the node's
//! placement `(parentId, sortKey)`, or its creation (which writes every key at once).
//! Revisions are stored as `u32` offsets from the load revision; nodes never written
//! since load carry no table at all.
use super::{Key, Store};
use std::collections::HashMap;
use std::sync::Arc;

/// Interned top-level props keys. Few and rarely new, so a version copies the table
/// when a batch adds a key; ids never change.
#[derive(Clone, Default)]
pub(crate) struct PropNames {
    ids: HashMap<Arc<str>, u32>,
    names: Vec<Arc<str>>,
}

/// One stamped property of a node.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Prop {
    Placement,
    Created,
    /// Interned top-level props key.
    Key(u32),
}

impl Prop {
    fn code(self) -> u32 {
        match self {
            Prop::Placement => 0,
            Prop::Created => 1,
            Prop::Key(k) => k + 2,
        }
    }

    fn decode(code: u32) -> Self {
        match code {
            0 => Prop::Placement,
            1 => Prop::Created,
            k => Prop::Key(k - 2),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    prop: u32,
    rev: u32,
}

/// Stamp table of one node, sorted by property code.
#[derive(Clone, Debug, Default)]
pub(crate) struct Stamps(Option<Box<[Entry]>>);

impl Stamps {
    /// A table for a node created at `rev`.
    pub(crate) fn created(rev: u32) -> Self {
        let entry = Entry {
            prop: Prop::Created.code(),
            rev,
        };
        Self(Some(Box::new([entry])))
    }

    fn entries(&self) -> &[Entry] {
        self.0.as_deref().unwrap_or(&[])
    }

    fn find(&self, prop: Prop) -> Option<u32> {
        let code = prop.code();
        let entries = self.entries();
        let at = entries.binary_search_by_key(&code, |e| e.prop).ok()?;
        Some(entries[at].rev)
    }

    /// Sets `prop` to `rev` (`None` removes it), returning the previous value.
    fn set(&mut self, prop: Prop, rev: Option<u32>) -> Option<u32> {
        let code = prop.code();
        // Rewriting an existing entry needs no reallocation.
        if let (Some(rev), Some(entries)) = (rev, self.0.as_deref_mut()) {
            if let Ok(at) = entries.binary_search_by_key(&code, |e| e.prop) {
                return Some(std::mem::replace(&mut entries[at].rev, rev));
            }
        }
        let mut entries: Vec<Entry> = self.0.take().map(Vec::from).unwrap_or_default();
        let old = match entries.binary_search_by_key(&code, |e| e.prop) {
            Ok(at) => {
                let old = entries[at].rev;
                match rev {
                    Some(rev) => entries[at].rev = rev,
                    None => drop(entries.remove(at)),
                }
                Some(old)
            }
            Err(at) => {
                if let Some(rev) = rev {
                    entries.insert(at, Entry { prop: code, rev });
                }
                None
            }
        };
        self.0 = (!entries.is_empty()).then(|| entries.into_boxed_slice());
        old
    }

    /// The revision offset `prop` was last written at, counting the creation.
    pub(crate) fn get(&self, prop: Prop) -> Option<u32> {
        let own = self.find(prop);
        let created = self.find(Prop::Created);
        own.max(created)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (Prop, u32)> + '_ {
        self.entries().iter().map(|e| (Prop::decode(e.prop), e.rev))
    }

    /// Estimated heap bytes of the table.
    pub(crate) fn bytes(&self) -> usize {
        match &self.0 {
            Some(entries) => crate::props::ALLOC + std::mem::size_of_val(&**entries),
            None => 0,
        }
    }
}

impl Store {
    /// Records that `prop` of `key` was written at revision offset `rev`.
    pub(crate) fn stamp(&mut self, key: Key, prop: Prop, rev: u32) {
        if self.slot(key).stamps.find(prop) == Some(rev) {
            return;
        }
        let slot = self.slot_mut(key);
        let before = slot.stamps.bytes();
        slot.stamps.set(prop, Some(rev));
        let after = slot.stamps.bytes();
        self.bytes = self.bytes + after - before;
    }

    /// Stamps a top-level props key by name.
    pub(crate) fn stamp_key(&mut self, key: Key, name: &str, rev: u32) {
        let id = match self.props.ids.get(name) {
            Some(&id) => id,
            None => {
                let names = Arc::make_mut(&mut self.props);
                let id = u32::try_from(names.names.len()).expect("props key count fits u32");
                let name: Arc<str> = name.into();
                names.names.push(name.clone());
                names.ids.insert(name, id);
                id
            }
        };
        let prop = Prop::Key(id);
        self.stamp(key, prop, rev);
    }

    pub(crate) fn prop_name(&self, prop: Prop) -> Option<&str> {
        match prop {
            Prop::Key(id) => Some(&self.props.names[id as usize]),
            _ => None,
        }
    }

    pub(crate) fn find_prop(&self, name: &str) -> Option<Prop> {
        self.props.ids.get(name).copied().map(Prop::Key)
    }
}
