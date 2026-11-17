//! The PostgreSQL schema that holds the runtime tables, chosen per deployment.
use crate::{Result, StorageError};
use std::{fmt, sync::Arc};

/// Runtime schema used when a deployment selects none.
pub const DEFAULT_DATABASE_SCHEMA: &str = "fagent_runtime_v1";

/// Longest identifier PostgreSQL keeps without truncation (NAMEDATALEN - 1).
const MAX_LEN: usize = 63;

/// A validated schema name: 1–63 lowercase ASCII letters, digits or `_`, starting with a
/// letter or `_`, never a reserved `pg_` name or `information_schema`. Such a name reads
/// the same quoted or unquoted, so the quoted form is safe to place in SQL text and in
/// `search_path`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatabaseSchema(Arc<str>);

impl DatabaseSchema {
    pub fn parse(name: &str) -> Result<Self> {
        let mut chars = name.chars();
        let first_ok = chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c == '_');
        let rest_ok = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !first_ok
            || !rest_ok
            || name.len() > MAX_LEN
            || name.starts_with("pg_")
            || name == "information_schema"
        {
            return Err(StorageError::validation(
                "schema",
                "Database schema must be 1-63 lowercase letters, digits or underscores, start with a letter or underscore and not be a system schema",
            ));
        }
        Ok(Self(name.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Double-quoted identifier; validation leaves nothing to escape.
    pub(super) fn quoted(&self) -> String {
        format!("\"{}\"", self.0)
    }
}

impl Default for DatabaseSchema {
    fn default() -> Self {
        Self(DEFAULT_DATABASE_SCHEMA.into())
    }
}

impl fmt::Display for DatabaseSchema {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_the_versioned_runtime_schema() {
        assert_eq!(DatabaseSchema::default().as_str(), "fagent_runtime_v1");
        assert_eq!(DatabaseSchema::default().quoted(), "\"fagent_runtime_v1\"");
    }

    #[test]
    fn accepts_plain_lowercase_identifiers() {
        for name in [
            "public",
            "fagent_runtime_v1",
            "_staging",
            "a",
            "user",
            &"x".repeat(63),
        ] {
            assert_eq!(DatabaseSchema::parse(name).unwrap().as_str(), name);
        }
    }

    #[test]
    fn rejects_names_that_need_quoting_or_are_reserved() {
        for name in [
            "",
            "Public",
            "1schema",
            "fagent-runtime",
            "fagent runtime",
            "public,pg_catalog",
            "public\"; DROP SCHEMA public; --",
            "sch\u{e9}ma",
            "pg_catalog",
            "pg_temp",
            "information_schema",
            &"x".repeat(64),
        ] {
            let error = DatabaseSchema::parse(name).unwrap_err();
            assert_eq!(error.code, "VALIDATION_FAILED", "{name:?}");
        }
    }
}
