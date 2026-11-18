//! `upsert_token` and `upsert_style` with `Candidate::apply`'s checks: a token is put
//! first and then every token is revalidated (`validate_tokens`, including the alias
//! graph); a style is checked (`check_style`) before it is put.
use super::Txn;
use crate::checks;
use fagent_scene_input::validate::{check_style, validate_tokens};
use fagent_scene_input::{Result, Style, Token};

impl Txn<'_> {
    pub(super) fn upsert_token(&mut self, token: &Token) -> Result<()> {
        checks::valid(&token.id)?;
        self.store.put_token(token.clone());
        validate_tokens(&*self.store)?;
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
