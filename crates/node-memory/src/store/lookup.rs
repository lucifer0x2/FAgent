//! The validators' read-only [`Lookup`] over one store version, so per-operation
//! property checks run on the batch's working state exactly as `Candidate::apply`
//! runs them on its maps. Every list keeps the order `Candidate` documents.
use super::{Key, Store};
use fagent_scene_input::validate::Lookup;
use fagent_scene_input::{Asset, Font, Node, Note, Style, Token};

impl Store {
    fn ids_of(&self, keys: impl Iterator<Item = Key>) -> Vec<&str> {
        let mut ids: Vec<&str> = keys.map(|k| self.node(k).id.as_str()).collect();
        ids.sort_unstable();
        ids
    }
}

impl Lookup for Store {
    fn node(&self, id: &str) -> Option<&Node> {
        self.get(id).map(|slot| slot.node.as_ref())
    }

    fn node_count(&self) -> usize {
        self.len()
    }

    fn pages(&self) -> Vec<&Node> {
        let keys = Store::children(self, None);
        keys.iter().map(|&k| Store::node(self, k)).collect()
    }

    fn children(&self, id: &str) -> Vec<&Node> {
        let keys = self.get(id).map_or(&[][..], |slot| &slot.children[..]);
        keys.iter().map(|&k| Store::node(self, k)).collect()
    }

    fn referencing(&self, key: &str) -> Vec<&str> {
        self.ids_of(Store::referencing(self, key))
    }

    fn token(&self, id: &str) -> Option<&Token> {
        self.res.tokens.get(id)
    }

    fn tokens(&self) -> Box<dyn Iterator<Item = &Token> + '_> {
        Box::new(self.res.tokens.values())
    }

    fn token_count(&self) -> usize {
        self.res.tokens.len()
    }

    fn theme_declared(&self, name: &str) -> bool {
        self.res.token_index.theme_declared(name)
    }

    fn named_themes(&self) -> usize {
        self.res.token_index.named_themes()
    }

    fn declared_themes(&self) -> Vec<&str> {
        self.res.token_index.declared_themes()
    }

    fn aliasers(&self, token: &str) -> Vec<&str> {
        self.res.token_index.aliasers(token).collect()
    }

    fn style(&self, id: &str) -> Option<&Style> {
        self.res.styles.get(id)
    }

    fn styles(&self) -> Box<dyn Iterator<Item = &Style> + '_> {
        Box::new(self.res.styles.values())
    }

    fn font(&self, id: &str) -> Option<&Font> {
        self.res.fonts.get(id)
    }

    fn fonts(&self) -> Box<dyn Iterator<Item = &Font> + '_> {
        Box::new(self.res.fonts.values())
    }

    fn asset(&self, id: &str) -> Option<&Asset> {
        self.res.assets.get(id)
    }

    fn note(&self, id: &str) -> Option<&Note> {
        self.res.note(id)
    }

    fn note_count(&self) -> usize {
        self.res.notes.len()
    }

    fn notes_on(&self, node: &str) -> Vec<&Note> {
        let ids = self.res.notes_on(node);
        ids.filter_map(|id| self.res.note(id)).collect()
    }

    fn component_users(&self, master: &str) -> Vec<&str> {
        self.ids_of(self.users(master))
    }

    fn incoming_links(&self, target: &str) -> Vec<&str> {
        self.ids_of(self.linking(target))
    }
}
