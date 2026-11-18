//! `upsert_token` and `upsert_style`. A style is checked (`check_style`) before it is
//! put, as `Candidate::apply` does. A token is put without the alias-graph check
//! `Candidate::apply` makes after every upsert: the whole token set is validated once,
//! in `finish`, when the batch upserted any token (see [`Txn::deferred`]).
use super::Txn;
use crate::checks;
use fagent_scene_input::validate::check_style;
use fagent_scene_input::{Result, Style, Token};

impl Txn<'_> {
    pub(super) fn upsert_token(&mut self, token: &Token) -> Result<()> {
        checks::valid(&token.id)?;
        self.store.put_token(token.clone());
        self.tokens.insert(token.id.clone());
        Ok(())
    }

    pub(super) fn upsert_style(&mut self, style: &Style) -> Result<()> {
        check_style(&*self.store, style)?;
        self.store.put_style(style.clone());
        self.styles.insert(style.id.clone());
        Ok(())
    }
}
