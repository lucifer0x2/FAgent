//! `upsert_token` and `upsert_style`, with `Candidate::apply`'s checks. A style is
//! checked (`check_style`) before it is put. A token is put, then the token set is
//! checked as `validate_tokens` would after the write, limited to what the write can
//! affect (`check_token_write`): the token, its aliasers and the alias paths through
//! it, not the whole set.
use super::Txn;
use crate::checks;
use fagent_scene_input::validate::{check_style, check_token_write};
use fagent_scene_input::{Result, Style, Token};

impl Txn<'_> {
    pub(super) fn upsert_token(&mut self, token: &Token) -> Result<()> {
        checks::valid(&token.id)?;
        let previous = self.store.put_token(token.clone());
        check_token_write(&*self.store, &token.id, previous.as_ref())?;
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
