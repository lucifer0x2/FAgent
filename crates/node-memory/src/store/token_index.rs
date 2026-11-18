//! Indexes over the token envelopes, for the per-upsert token checks: how many tokens
//! declare each theme name, and which tokens alias each token. Envelopes are read
//! leniently (`theme_names`, `alias_targets`), so an invalid token is indexed as the
//! `Candidate` scans read it. Both maps are shared between versions and copied on the
//! first token write of a batch.
use fagent_scene_input::validate::{alias_targets, theme_names};
use fagent_scene_input::Token;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

#[derive(Clone, Default)]
pub(crate) struct TokenIndex {
    /// Theme name -> number of tokens whose `themes` object declares it.
    themes: Arc<BTreeMap<String, usize>>,
    /// Alias target id -> ids of the tokens with a branch aliasing it.
    aliasers: Arc<HashMap<String, BTreeSet<String>>>,
}

impl TokenIndex {
    pub(crate) fn build<'a>(tokens: impl Iterator<Item = &'a Token>) -> Self {
        let mut index = Self::default();
        for token in tokens {
            index.add(token);
        }
        index
    }

    /// Re-indexes a token written over `old`.
    pub(crate) fn replace(&mut self, old: Option<&Token>, new: &Token) {
        if let Some(old) = old {
            self.remove(old);
        }
        self.add(new);
    }

    fn add(&mut self, token: &Token) {
        let themes = Arc::make_mut(&mut self.themes);
        for name in theme_names(token) {
            *themes.entry(name.to_owned()).or_default() += 1;
        }
        let aliasers = Arc::make_mut(&mut self.aliasers);
        for target in alias_targets(token) {
            let set = aliasers.entry(target.to_owned()).or_default();
            set.insert(token.id.clone());
        }
    }

    fn remove(&mut self, token: &Token) {
        let themes = Arc::make_mut(&mut self.themes);
        for name in theme_names(token) {
            if let Some(count) = themes.get_mut(name) {
                *count -= 1;
                if *count == 0 {
                    themes.remove(name);
                }
            }
        }
        let aliasers = Arc::make_mut(&mut self.aliasers);
        for target in alias_targets(token) {
            if let Some(set) = aliasers.get_mut(target) {
                set.remove(&token.id);
                if set.is_empty() {
                    aliasers.remove(target);
                }
            }
        }
    }

    pub(crate) fn theme_declared(&self, name: &str) -> bool {
        self.themes.contains_key(name)
    }

    /// Distinct declared theme names, `default` excluded.
    pub(crate) fn named_themes(&self) -> usize {
        self.themes.len() - usize::from(self.themes.contains_key("default"))
    }

    /// Ids of the tokens aliasing `token`, in id order.
    pub(crate) fn aliasers(&self, token: &str) -> impl Iterator<Item = &str> {
        let ids = self.aliasers.get(token).into_iter().flatten();
        ids.map(String::as_str)
    }
}
