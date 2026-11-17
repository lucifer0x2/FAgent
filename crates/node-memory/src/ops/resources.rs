//! `upsert_token` and `upsert_style`: insert or replace by id. Only the id is checked
//! here; token graphs and style values stay with the full validator.
use super::Txn;
use crate::checks;
use fagent_scene_input::{Result, Style, Token};

impl Txn<'_> {
    pub(super) fn upsert_token(&mut self, token: &Token) -> Result<()> {
        checks::valid(&token.id)?;
        self.store.put_token(token.clone(), &mut self.log);
        self.tokens.insert(token.id.clone());
        Ok(())
    }

    pub(super) fn upsert_style(&mut self, style: &Style) -> Result<()> {
        checks::valid(&style.id)?;
        self.store.put_style(style.clone(), &mut self.log);
        self.styles.insert(style.id.clone());
        Ok(())
    }
}
