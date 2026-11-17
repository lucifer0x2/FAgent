//! Snapshot resources held next to the nodes: tokens, styles, assets, fonts and notes,
//! each by id (the stored `ORDER BY id`), plus notes by anchor node. Writes are logged
//! with the replaced value so a failed batch restores them exactly.
use super::{Log, Store, Undo};
use fagent_scene_input::{Asset, Font, Note, Snapshot, Style, Token};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Default)]
pub(crate) struct Resources {
    pub tokens: BTreeMap<String, Token>,
    pub styles: BTreeMap<String, Style>,
    pub assets: BTreeMap<String, Asset>,
    pub fonts: BTreeMap<String, Font>,
    pub notes: BTreeMap<String, Note>,
    /// Note ids by anchor node id.
    anchors: HashMap<String, BTreeSet<String>>,
}

/// Inverse of one resource write: the value before it (`None`: did not exist).
#[derive(Debug)]
pub(crate) enum ResUndo {
    Token(String, Option<Box<Token>>),
    Style(String, Option<Box<Style>>),
    Note(String, Option<Box<Note>>),
}

fn by_id<T>(items: Vec<T>, id: impl Fn(&T) -> &String) -> BTreeMap<String, T> {
    items.into_iter().map(|x| (id(&x).clone(), x)).collect()
}

impl Resources {
    /// Moves the resources out of `snapshot`, leaving its vectors empty.
    pub(crate) fn take(snapshot: &mut Snapshot) -> Self {
        let mut res = Self {
            tokens: by_id(std::mem::take(&mut snapshot.tokens), |t| &t.id),
            styles: by_id(std::mem::take(&mut snapshot.styles), |s| &s.id),
            assets: by_id(std::mem::take(&mut snapshot.assets), |a| &a.id),
            fonts: by_id(std::mem::take(&mut snapshot.fonts), |f| &f.id),
            notes: by_id(std::mem::take(&mut snapshot.notes), |n| &n.id),
            anchors: HashMap::new(),
        };
        for note in res.notes.values() {
            let set = res.anchors.entry(note.node_id.clone()).or_default();
            set.insert(note.id.clone());
        }
        res
    }

    /// Copies the resources into `snapshot`, in id order.
    pub(crate) fn fill(&self, snapshot: &mut Snapshot) {
        snapshot.tokens = self.tokens.values().cloned().collect();
        snapshot.styles = self.styles.values().cloned().collect();
        snapshot.assets = self.assets.values().cloned().collect();
        snapshot.fonts = self.fonts.values().cloned().collect();
        snapshot.notes = self.notes.values().cloned().collect();
    }

    /// Ids of the notes anchored to node `id`, in id order.
    pub(crate) fn notes_on<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        let notes = self.anchors.get(id).into_iter().flatten();
        notes.map(String::as_str)
    }

    fn put_note(&mut self, note: Option<Note>, id: &str) -> Option<Note> {
        let old = match note {
            Some(note) => self.notes.insert(id.to_owned(), note),
            None => self.notes.remove(id),
        };
        if let Some(old) = &old {
            if let Some(set) = self.anchors.get_mut(&old.node_id) {
                set.remove(&old.id);
                if set.is_empty() {
                    self.anchors.remove(&old.node_id);
                }
            }
        }
        if let Some(new) = self.notes.get(id) {
            let set = self.anchors.entry(new.node_id.clone()).or_default();
            set.insert(new.id.clone());
        }
        old
    }

    pub(super) fn undo(&mut self, entry: ResUndo) {
        match entry {
            ResUndo::Token(id, old) => match old {
                Some(token) => drop(self.tokens.insert(id, *token)),
                None => drop(self.tokens.remove(&id)),
            },
            ResUndo::Style(id, old) => match old {
                Some(style) => drop(self.styles.insert(id, *style)),
                None => drop(self.styles.remove(&id)),
            },
            ResUndo::Note(id, old) => drop(self.put_note(old.map(|n| *n), &id)),
        }
    }
}

impl Store {
    pub(crate) fn put_token(&mut self, token: Token, log: &mut Log) {
        let id = token.id.clone();
        let old = self.res.tokens.insert(id.clone(), token).map(Box::new);
        log.push(Undo::Res(ResUndo::Token(id, old)));
    }

    pub(crate) fn put_style(&mut self, style: Style, log: &mut Log) {
        let id = style.id.clone();
        let old = self.res.styles.insert(id.clone(), style).map(Box::new);
        log.push(Undo::Res(ResUndo::Style(id, old)));
    }

    /// Inserts or replaces (`Some`) or removes (`None`) note `id`.
    pub(crate) fn put_note(&mut self, id: &str, note: Option<Note>, log: &mut Log) {
        let old = self.res.put_note(note, id).map(Box::new);
        log.push(Undo::Res(ResUndo::Note(id.to_owned(), old)));
    }
}
