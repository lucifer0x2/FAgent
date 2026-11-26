//! `FAGENT_MAX_NOTES`, `FAGENT_MAX_NOTE_TEXT`, `FAGENT_MAX_NOTE_REPLIES` and
//! `FAGENT_MAX_NOTE_AUTHOR`: raise the note limits from their defaults up to the ceilings.
use fagent_storage::NoteLimits;
use std::env::{self, VarError};

/// The limits from the environment; an unset or empty variable keeps the default.
pub(super) fn note_limits() -> Result<NoteLimits, String> {
    parse(|name| env::var(name))
}

fn parse(lookup: impl Fn(&str) -> Result<String, VarError>) -> Result<NoteLimits, String> {
    let (low, high) = (NoteLimits::DEFAULT, NoteLimits::CEILING);
    let read = |name: &str, low: usize, high: usize| {
        let invalid = || format!("{name} must be a whole number from {low} to {high}");
        match lookup(name) {
            Err(VarError::NotPresent) => Ok(low),
            Ok(value) if value.is_empty() => Ok(low),
            Ok(value) if value.len() <= 9 && value.bytes().all(|b| b.is_ascii_digit()) => value
                .parse()
                .ok()
                .filter(|n| (low..=high).contains(n))
                .ok_or_else(invalid),
            _ => Err(invalid()),
        }
    };
    Ok(NoteLimits {
        notes: read("FAGENT_MAX_NOTES", low.notes, high.notes)?,
        text: read("FAGENT_MAX_NOTE_TEXT", low.text, high.text)?,
        replies: read("FAGENT_MAX_NOTE_REPLIES", low.replies, high.replies)?,
        author: read("FAGENT_MAX_NOTE_AUTHOR", low.author, high.author)?,
    })
}

#[cfg(test)]
mod tests {
    use super::parse;
    use fagent_storage::NoteLimits;
    use std::env::VarError;

    fn with(pairs: &[(&str, &str)]) -> Result<NoteLimits, String> {
        parse(|name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string())
                .ok_or(VarError::NotPresent)
        })
    }

    #[test]
    fn unset_or_empty_keeps_the_defaults() {
        assert_eq!(with(&[]), Ok(NoteLimits::DEFAULT));
        assert_eq!(
            with(&[("FAGENT_MAX_NOTE_TEXT", "")]),
            Ok(NoteLimits::DEFAULT)
        );
    }

    #[test]
    fn raises_each_limit_up_to_its_ceiling() {
        let limits = with(&[
            ("FAGENT_MAX_NOTES", "50000"),
            ("FAGENT_MAX_NOTE_TEXT", "100000"),
            ("FAGENT_MAX_NOTE_REPLIES", "500"),
            ("FAGENT_MAX_NOTE_AUTHOR", "128"),
        ]);
        let expected = NoteLimits {
            notes: 50_000,
            text: 100_000,
            replies: 500,
            author: 128,
        };
        assert_eq!(limits, Ok(expected));
    }

    #[test]
    fn rejects_values_below_the_default_over_the_ceiling_or_not_digits() {
        for (name, value) in [
            ("FAGENT_MAX_NOTES", "100"),
            ("FAGENT_MAX_NOTES", "1000001"),
            ("FAGENT_MAX_NOTE_TEXT", "1999"),
            ("FAGENT_MAX_NOTE_TEXT", "+5000"),
            ("FAGENT_MAX_NOTE_REPLIES", "1001"),
            ("FAGENT_MAX_NOTE_REPLIES", " 200"),
            ("FAGENT_MAX_NOTE_AUTHOR", "1e2"),
            ("FAGENT_MAX_NOTE_AUTHOR", "99999999999999999999"),
        ] {
            let error = with(&[(name, value)]).unwrap_err();
            assert!(error.starts_with(name), "{name}={value}: {error}");
        }
    }
}
