//! Snapshot resources held next to the nodes: tokens, styles, assets, fonts and notes,
//! each by id (the stored `ORDER BY id`), plus notes by anchor node. Each map is
//! shared between versions and copied on the first write of a batch; notes are
//! shared individually, so copying the note map never copies note text. Token
//! writes keep the token index (`token_index`) in step.
use super::token_index::TokenIndex;
use super::Store;
use fagent_scene_input::{Asset, Font, Note, Snapshot, Style, Token};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

/// Note ids of one anchor, borrowed from the anchor index only (not from the id).
pub(crate) type NoteIds<'a> = std::iter::Map<
    std::iter::Flatten<std::option::IntoIter<&'a BTreeSet<String>>>,
    fn(&String) -> &str,
>;

#[derive(Clone, Default)]
pub(crate) struct Resources {
    pub tokens: Arc<BTreeMap<String, Token>>,
    pub styles: Arc<BTreeMap<String, Style>>,
    pub assets: Arc<BTreeMap<String, Asset>>,
    pub fonts: Arc<BTreeMap<String, Font>>,
    pub notes: Arc<BTreeMap<String, Arc<Note>>>,
    /// Note ids by anchor node id.
    anchors: Arc<HashMap<String, BTreeSet<String>>>,
    /// Theme names and alias edges of `tokens`.
    pub token_index: TokenIndex,
}

fn by_id<T>(items: Vec<T>, id: impl Fn(&T) -> &String) -> Arc<BTreeMap<String, T>> {
    Arc::new(items.into_iter().map(|x| (id(&x).clone(), x)).collect())
}

impl Resources {
    /// Moves the resources out of `snapshot`, leaving its vectors empty.
    pub(crate) fn take(snapshot: &mut Snapshot) -> Self {
        let notes = std::mem::take(&mut snapshot.notes);
        let mut anchors: HashMap<String, BTreeSet<String>> = HashMap::new();
        for note in &notes {
            let set = anchors.entry(note.node_id.clone()).or_default();
            set.insert(note.id.clone());
        }
        let tokens = by_id(std::mem::take(&mut snapshot.tokens), |t| &t.id);
        Self {
            token_index: TokenIndex::build(tokens.values()),
            tokens,
            styles: by_id(std::mem::take(&mut snapshot.styles), |s| &s.id),
            assets: by_id(std::mem::take(&mut snapshot.assets), |a| &a.id),
            fonts: by_id(std::mem::take(&mut snapshot.fonts), |f| &f.id),
            notes: Arc::new(
                notes
                    .into_iter()
                    .map(|n| (n.id.clone(), Arc::new(n)))
                    .collect(),
            ),
            anchors: Arc::new(anchors),
        }
    }

    /// Copies tokens, styles, assets and fonts into `snapshot`, in id order.
    pub(crate) fn fill_resources(&self, snapshot: &mut Snapshot) {
        snapshot.tokens = self.tokens.values().cloned().collect();
        snapshot.styles = self.styles.values().cloned().collect();
        snapshot.assets = self.assets.values().cloned().collect();
        snapshot.fonts = self.fonts.values().cloned().collect();
    }

    /// Copies every resource, notes included, into `snapshot`, in id order.
    pub(crate) fn fill(&self, snapshot: &mut Snapshot) {
        self.fill_resources(snapshot);
        snapshot.notes = self.notes.values().map(|n| n.as_ref().clone()).collect();
    }

    /// Ids of the notes anchored to node `id`, in id order.
    pub(crate) fn notes_on(&self, id: &str) -> NoteIds<'_> {
        let notes = self.anchors.get(id).into_iter().flatten();
        notes.map(String::as_str as fn(&String) -> &str)
    }

    pub(crate) fn note(&self, id: &str) -> Option<&Note> {
        self.notes.get(id).map(Arc::as_ref)
    }

    fn put_note(&mut self, note: Option<Note>, id: &str) {
        let notes = Arc::make_mut(&mut self.notes);
        let old = match note {
            Some(note) => notes.insert(id.to_owned(), Arc::new(note)),
            None => notes.remove(id),
        };
        let anchors = Arc::make_mut(&mut self.anchors);
        if let Some(old) = &old {
            if let Some(set) = anchors.get_mut(&old.node_id) {
                set.remove(&old.id);
                if set.is_empty() {
                    anchors.remove(&old.node_id);
                }
            }
        }
        if let Some(new) = notes.get(id) {
            let set = anchors.entry(new.node_id.clone()).or_default();
            set.insert(new.id.clone());
        }
    }
}

impl Store {
    /// Inserts or replaces a token; returns the token it replaced.
    pub(crate) fn put_token(&mut self, token: Token) -> Option<Token> {
        let id = token.id.clone();
        let tokens = Arc::make_mut(&mut self.res.tokens);
        let old = tokens.insert(id.clone(), token);
        self.res.token_index.replace(old.as_ref(), &tokens[&id]);
        old
    }

    pub(crate) fn put_style(&mut self, style: Style) {
        Arc::make_mut(&mut self.res.styles).insert(style.id.clone(), style);
    }

    /// Inserts or replaces (`Some`) or removes (`None`) note `id`.
    pub(crate) fn put_note(&mut self, id: &str, note: Option<Note>) {
        self.res.put_note(note, id);
    }
}
