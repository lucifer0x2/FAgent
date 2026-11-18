//! What a schema rejection reports, and which `oneOf` variant it blames when none matches.
use crate::error::ApiError;
use serde_json::Value;
use std::cmp::Reverse;

/// Longest object key echoed in `field` and the message; callers choose key names.
const KEY_ECHO: usize = 64;

/// One step of a field path: an object key or an array index.
#[derive(Clone, PartialEq)]
pub(super) enum Step {
    Key(String),
    Index(usize),
}

pub(super) enum Kind {
    /// A `const` did not match, or a required `const` field is missing: "not this variant".
    Discriminator { expected: Value, missing: bool },
    /// The value has none of these JSON types.
    Type(Vec<String>),
    /// A field the object does not accept.
    Unknown,
    /// Every required field missing from the object; `path` names the first.
    Required(Vec<String>),
    /// A constraint on a value of an accepted type: range, length, pattern, enum or items.
    Value,
}

pub(super) struct Failure {
    pub path: Vec<Step>,
    pub reason: String,
    pub kind: Kind,
}

impl Failure {
    pub fn new(path: &[Step], reason: impl Into<String>, kind: Kind) -> Self {
        Self {
            path: path.to_vec(),
            reason: reason.into(),
            kind,
        }
    }
    pub fn value(path: &[Step], reason: impl Into<String>) -> Self {
        Self::new(path, reason, Kind::Value)
    }
    fn field(&self) -> String {
        let mut out = String::new();
        for step in &self.path {
            match step {
                Step::Key(key) => {
                    if !out.is_empty() {
                        out.push('.');
                    }
                    out.extend(key.chars().take(KEY_ECHO));
                    if key.chars().nth(KEY_ECHO).is_some() {
                        out.push('…');
                    }
                }
                Step::Index(i) => out.push_str(&format!("[{i}]")),
            }
        }
        out
    }
    pub fn into_error(self) -> ApiError {
        const PREFIX: &str = "Arguments do not match the published tool schema";
        let field = self.field();
        let message = if field.is_empty() {
            format!("{PREFIX}: {}", self.reason)
        } else {
            format!("{PREFIX} at {field}: {}", self.reason)
        };
        let mut error = ApiError::invalid(&message);
        if let [Step::Key(key), Step::Index(i), ..] = self.path.as_slice() {
            if key == "operations" {
                error.0.op_index = Some(*i);
            }
        }
        if !field.is_empty() {
            error.0.field = Some(field);
        }
        error
    }
}

/// How close a variant came to matching, compared in order: it has the right kind (not a
/// discriminator miss); it accepts more of the fields sent; it failed deeper in the value;
/// it failed on a value rather than a missing field, an unknown field or a wrong type.
type Score = (bool, Reverse<usize>, usize, u8);

fn score(choice: &Value, value: &Value, failure: &Failure) -> Score {
    let strangers = match (value.as_object(), choice["additionalProperties"] == false) {
        (Some(object), true) => object
            .keys()
            .filter(|key| choice["properties"].get(key.as_str()).is_none())
            .count(),
        _ => 0,
    };
    let kind = match failure.kind {
        Kind::Value => 3,
        Kind::Required(_) => 2,
        Kind::Unknown => 1,
        Kind::Type(_) | Kind::Discriminator { .. } => 0,
    };
    let right_kind = !matches!(failure.kind, Kind::Discriminator { .. });
    (right_kind, Reverse(strangers), failure.path.len(), kind)
}

/// The failure to report when no variant of a `oneOf` at `path` matched `value`. Variants
/// that tie name what they need together: the accepted kinds, types or field sets.
pub(super) fn blame(path: &[Step], value: &Value, failures: Vec<(&Value, Failure)>) -> Failure {
    let scored: Vec<(Score, Failure)> = failures
        .into_iter()
        .map(|(choice, failure)| (score(choice, value, &failure), failure))
        .collect();
    let Some(top) = scored.iter().map(|(s, _)| *s).max() else {
        return Failure::value(path, "matches no accepted form");
    };
    let mut tied: Vec<Failure> = scored
        .into_iter()
        .filter(|(s, _)| *s == top)
        .map(|(_, f)| f)
        .collect();
    let first = tied.remove(0);
    match &first.kind {
        Kind::Discriminator { missing, .. } => {
            let kinds: Vec<Value> = std::iter::once(&first)
                .chain(&tied)
                .filter_map(|f| match &f.kind {
                    Kind::Discriminator { expected, .. } if f.path == first.path => {
                        Some(expected.clone())
                    }
                    _ => None,
                })
                .collect();
            let lead = if *missing {
                "is required: one of"
            } else {
                "must be one of"
            };
            Failure::value(&first.path, format!("{lead} {}", list(&kinds)))
        }
        Kind::Type(_) => {
            let mut types: Vec<String> = Vec::new();
            for failure in std::iter::once(&first).chain(&tied) {
                if let Kind::Type(names) = &failure.kind {
                    if failure.path == first.path {
                        for name in names {
                            if !types.contains(name) {
                                types.push(name.clone());
                            }
                        }
                    }
                }
            }
            let reason = format!("must be of type {}", types.join(" or "));
            Failure::new(&first.path, reason, Kind::Type(types))
        }
        Kind::Required(_) if !tied.is_empty() => {
            // Variants that differ only in which fields they need: list each smallest set
            // (a variant taking both inputs adds nothing to "send one of").
            let parent = &first.path[..first.path.len() - 1];
            let sets: Vec<&Vec<String>> = std::iter::once(&first)
                .chain(&tied)
                .filter_map(|f| match &f.kind {
                    Kind::Required(missing) => Some(missing),
                    _ => None,
                })
                .collect();
            let covers = |a: &Vec<String>, b: &Vec<String>| b.iter().all(|k| a.contains(k));
            let mut forms: Vec<String> = Vec::new();
            for set in &sets {
                let smaller = sets
                    .iter()
                    .any(|other| other.len() < set.len() && covers(set, other));
                let form = set.join("+");
                if !smaller && !forms.contains(&form) {
                    forms.push(form);
                }
            }
            if forms.len() == 1 {
                return first;
            }
            let reason = format!("is missing fields; send one of {}", forms.join(", "));
            Failure::new(parent, reason, Kind::Required(forms))
        }
        _ => first,
    }
}

/// An enum's choices, shortened when long.
pub(super) fn list(choices: &[Value]) -> String {
    const SHOWN: usize = 12;
    let mut shown: Vec<String> = choices.iter().take(SHOWN).map(Value::to_string).collect();
    if choices.len() > SHOWN {
        shown.push(format!("… ({} in all)", choices.len()));
    }
    shown.join(", ")
}
