use super::{token_graph::validate_token_graph, values::*, Candidate, Lookup};
use crate::{error::*, model::Token};
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) const MAX_ALIAS_DEPTH: usize = 32;
pub(super) const MAX_THEMES: usize = 32;
pub(super) const MAX_TOKENS: usize = 4096;

/// A token envelope is deliberately shallow: branches cannot contain envelopes.
pub(super) fn branches(token: &Token) -> Result<Vec<(&str, &Value)>> {
    let value = &token.value;
    if !value.is_object() || value.get("aliasId").is_some() {
        return Ok(vec![("default", value)]);
    }
    let object = value.as_object().unwrap();
    if object.len() != 2 || !object.contains_key("default") {
        return Err(StorageError::validation(
            "token.value",
            "Expected a literal, aliasId or default/themes envelope",
        ));
    }
    let themes = object
        .get("themes")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            StorageError::validation(
                "token.themes",
                "Themes must be a bounded name-to-value object",
            )
        })?;
    if themes.len() > MAX_THEMES {
        return Err(StorageError::validation(
            "token.themes",
            "At most 32 named themes are supported",
        ));
    }
    let mut out = vec![("default", &object["default"])];
    for (name, branch) in themes {
        if name == "default" || name.len() > 64 || !valid_id(name) {
            return Err(StorageError::validation("token.themes", "Theme names require 1–64 ASCII letters, digits, underscores or hyphens; default is reserved"));
        }
        out.push((name.as_str(), branch));
    }
    Ok(out)
}

pub(super) fn alias(value: &Value) -> Result<Option<&str>> {
    if !value.is_object() {
        return Ok(None);
    }
    let object = value.as_object().unwrap();
    let target = object
        .get("aliasId")
        .and_then(Value::as_str)
        .filter(|id| valid_id(id));
    if object.len() != 1 || target.is_none() {
        return Err(StorageError::validation(
            "token.value",
            "A token branch must be a literal or exact aliasId reference",
        ));
    }
    Ok(target)
}

/// Theme names a token's `themes` object declares, read leniently (any envelope, valid
/// or not): the names `Lookup::theme_declared` matches. On a valid token they are
/// exactly its named branches.
pub fn theme_names(token: &Token) -> impl Iterator<Item = &str> {
    let themes = token.value.get("themes").and_then(Value::as_object);
    themes
        .into_iter()
        .flat_map(|t| t.keys().map(String::as_str))
}

/// Alias targets of a token's branches, read leniently: an invalid envelope or branch
/// contributes nothing. On a valid token they are exactly its alias edges.
pub fn alias_targets(token: &Token) -> BTreeSet<&str> {
    let branches = branches(token).unwrap_or_default();
    let targets = branches
        .into_iter()
        .filter_map(|(_, v)| alias(v).ok().flatten());
    targets.collect()
}

/// The checks `validate_tokens` makes on one token, in its order; returns the
/// token's branches.
pub(super) fn check_token<'a>(
    lookup: &dyn Lookup,
    token: &'a Token,
) -> Result<Vec<(&'a str, &'a Value)>> {
    id(&token.id)?;
    if token.name.len() > 256 {
        return Err(StorageError::validation(
            "token.name",
            "Name exceeds 256 bytes",
        ));
    }
    if !matches!(token.token_type.as_str(), "color" | "dimension" | "number") {
        return Err(StorageError::validation(
            "token.type",
            "Only color, dimension and number tokens are supported",
        ));
    }
    let branches = branches(token)?;
    for (_, value) in &branches {
        if let Some(target_id) = alias(value)? {
            let target = lookup.token(target_id).ok_or_else(|| {
                StorageError::validation(
                    "token.aliasId",
                    "Alias target does not exist; forward references are not supported",
                )
            })?;
            if target.token_type != token.token_type {
                return Err(StorageError::validation(
                    "token.aliasId",
                    "Alias target must have the same token type",
                ));
            }
        } else {
            literal(value, &token.token_type)?;
        }
    }
    Ok(branches)
}

pub(super) fn too_many_tokens() -> StorageError {
    StorageError::validation("tokens", "At most 4096 tokens are supported")
}

pub(super) fn too_many_themes() -> StorageError {
    StorageError::validation(
        "token.themes",
        "At most 32 named themes per project are supported",
    )
}

/// Check every token in id order, then the alias graph; returns the declared themes.
pub fn validate_tokens(lookup: &dyn Lookup) -> Result<BTreeSet<String>> {
    if lookup.token_count() > MAX_TOKENS {
        return Err(too_many_tokens());
    }
    let mut themes = BTreeSet::from(["default".into()]);
    for token in lookup.tokens() {
        for (theme, _) in check_token(lookup, token)? {
            themes.insert(theme.into());
        }
    }
    if themes.len() > MAX_THEMES + 1 {
        return Err(too_many_themes());
    }
    validate_token_graph(lookup)?;
    Ok(themes)
}

/// The literal value of token `id` at `theme`, following aliases.
pub fn token_value(lookup: &dyn Lookup, id: &str, theme: &str) -> Result<Value> {
    let mut current = id;
    let mut seen = BTreeSet::new();
    for _ in 0..=MAX_ALIAS_DEPTH {
        if !seen.insert(current) {
            return Err(StorageError::validation(
                "token.aliasId",
                "Token alias cycle detected",
            ));
        }
        let token = lookup.token(current).ok_or_else(|| {
            StorageError::validation("token.aliasId", "Alias target does not exist")
        })?;
        let choices = branches(token)?;
        let value = choices
            .iter()
            .find(|(name, _)| *name == theme)
            .unwrap_or(&choices[0])
            .1;
        if let Some(target) = alias(value)? {
            current = target;
        } else {
            literal(value, &token.token_type)?;
            return Ok(value.clone());
        }
    }
    Err(StorageError::validation(
        "token.aliasId",
        "Token alias depth exceeds 32",
    ))
}

impl Candidate {
    pub(super) fn validate_tokens(&self) -> Result<BTreeSet<String>> {
        validate_tokens(self)
    }

    pub(super) fn select_theme(&mut self, theme: &str) -> Result<()> {
        if !self.validate_tokens()?.contains(theme) {
            return Err(unsupported_theme());
        }
        self.theme = theme.into();
        Ok(())
    }

    pub(super) fn token_value(&self, id: &str, theme: &str) -> Result<Value> {
        token_value(self, id, theme)
    }
}

pub(super) fn unsupported_theme() -> StorageError {
    let mut error = StorageError::new(
        "UNSUPPORTED_THEME",
        "Requested design theme is not declared by any token",
    );
    error.field = Some("theme".into());
    error
}
