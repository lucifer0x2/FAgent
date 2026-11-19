//! What a schema rejection reports, and which `oneOf` variant it blames when none matches.
use crate::error::ApiError;
use serde_json::Value;
use std::cmp::Reverse;

/// Longest object key echoed in `field` and the message; callers choose key names.
const KEY_ECHO: usize = 64;

/// One step of a field path: an object key or an array index.
#[derive(Clone, PartialEq)]
pub(super) enum Step {
    /// At most `KEY_ECHO + 1` characters: enough to echo the key and to know it was cut.
    Key(String),
    Index(usize),
}

impl Step {
    /// A key step that never copies more of a caller-chosen key than can be echoed.
    pub fn key(key: &str) -> Self {
        Self::Key(key.chars().take(KEY_ECHO + 1).collect())
    }
    /// A key is written bare unless it is empty or could read as several steps.
    fn write(&self, out: &mut String) {
        match self {
            Step::Index(i) => out.push_str(&format!("[{i}]")),
            Step::Key(key) => {
                let mut echoed: String = key.chars().take(KEY_ECHO).collect();
                if key.chars().nth(KEY_ECHO).is_some() {
                    echoed.push('…');
                }
                // Keys with control characters are quoted too, so a path never spans lines.
                let bare = !echoed.is_empty()
                    && !echoed
                        .chars()
                        .any(|c| c.is_control() || matches!(c, '.' | '[' | ']' | '"'));
                if bare {
                    if !out.is_empty() {
                        out.push('.');
                    }
                    out.push_str(&echoed);
                } else {
                    // JSON string escaping covers U+0000–U+001F, `"` and `\`; the C1 controls
                    // U+007F–U+009F (U+0085 is a line break) are escaped here as `\u00XX`.
                    out.push('[');
                    for c in Value::String(echoed).to_string().chars() {
                        if c.is_control() {
                            out.push_str(&format!("\\u{:04x}", c as u32));
                        } else {
                            out.push(c);
                        }
                    }
                    out.push(']');
                }
            }
        }
    }
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
            step.write(&mut out);
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

/// How close a variant came to matching, compared in order: it has the kind the caller
/// named (the operation `op` is their stated intent, so a variant of another kind never
/// wins on field coverage); it accepts more of the fields sent; it failed deeper in the
/// value; it failed on a value rather than a missing field, an unknown field or a wrong type.
type Score = (bool, Reverse<usize>, usize, u8);

/// How many of the fields sent this variant does not accept. A variant that cannot be an
/// object accepts none of them; a variant that is itself a `oneOf` counts as its best
/// alternative.
fn strangers(choice: &Value, value: &Value) -> usize {
    let Some(object) = value.as_object() else {
        return 0;
    };
    if let Some(alternatives) = choice["oneOf"].as_array() {
        if choice.get("type").is_none() && choice.get("properties").is_none() {
            return alternatives
                .iter()
                .map(|alternative| strangers(alternative, value))
                .min()
                .unwrap_or(0);
        }
    }
    if !takes_object(choice) {
        return object.len();
    }
    if choice["additionalProperties"] != false {
        return 0;
    }
    object
        .keys()
        .filter(|key| choice["properties"].get(key.as_str()).is_none())
        .count()
}

fn takes_object(choice: &Value) -> bool {
    match &choice["type"] {
        Value::String(t) => t == "object",
        Value::Array(types) => types.iter().any(|t| t == "object"),
        _ => true,
    }
}

/// Whether the form `choice` accepts key `last` of the object at `rel` below it. A step it
/// cannot follow (a `oneOf` on the way) counts as accepted, so the key is still reported
/// as conflicting rather than unknown.
fn accepts(choice: &Value, rel: &[Step], last: &Step) -> bool {
    let Some((step, rest)) = rel.split_first() else {
        let Step::Key(key) = last else {
            return true;
        };
        if let Some(alternatives) = choice["oneOf"].as_array() {
            if choice.get("type").is_none() && choice.get("properties").is_none() {
                return alternatives.iter().any(|a| accepts(a, rel, last));
            }
        }
        return takes_object(choice)
            && (choice["properties"].get(key.as_str()).is_some()
                || choice["additionalProperties"] != false);
    };
    if choice.get("oneOf").is_some() {
        return true;
    }
    let inner = match step {
        Step::Key(key) => match choice["properties"].get(key.as_str()) {
            Some(inner) => inner,
            None => &choice["additionalProperties"],
        },
        Step::Index(_) => &choice["items"],
    };
    match inner {
        Value::Object(_) => accepts(inner, rest, last),
        _ => true,
    }
}

fn score(choice: &Value, value: &Value, failure: &Failure) -> Score {
    let strangers = strangers(choice, value);
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
    let scored: Vec<(Score, &Value, Failure)> = failures
        .into_iter()
        .map(|(choice, failure)| (score(choice, value, &failure), choice, failure))
        .collect();
    let Some(top) = scored.iter().map(|(s, _, _)| *s).max() else {
        return Failure::value(path, "matches no accepted form");
    };
    let (choices, mut tied): (Vec<&Value>, Vec<Failure>) = scored
        .into_iter()
        .filter(|(s, _, _)| *s == top)
        .map(|(_, c, f)| (c, f))
        .unzip();
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
            let forms_text = forms.join(", ");
            // A field every form needs is the one to name (`projectId`), with the forms.
            let common = sets[0]
                .iter()
                .find(|key| sets.iter().all(|set| set.contains(key)));
            match common {
                Some(key) => {
                    let at = [parent, &[Step::key(key)]].concat();
                    let reason = format!("is required; then send one of {forms_text}");
                    Failure::new(&at, reason, Kind::Required(forms))
                }
                None => {
                    let reason = format!("is missing fields; send one of {forms_text}");
                    Failure::new(parent, reason, Kind::Required(forms))
                }
            }
        }
        Kind::Unknown if !tied.is_empty() => {
            // Each form rejects a field another form takes (two sources, say): name them all.
            // A field no tied form takes is simply unknown, and is reported on its own.
            let parent = &first.path[..first.path.len() - 1];
            let rel = parent.get(path.len()..).unwrap_or_default();
            let mut names: Vec<String> = Vec::new();
            let mut unknown = None;
            for (index, failure) in std::iter::once(&first).chain(&tied).enumerate() {
                let same_parent = failure.path.len() == first.path.len()
                    && failure.path[..failure.path.len() - 1] == *parent;
                if let (Kind::Unknown, true, Some(last)) =
                    (&failure.kind, same_parent, failure.path.last())
                {
                    if !choices.iter().any(|choice| accepts(choice, rel, last)) {
                        unknown.get_or_insert(index);
                        continue;
                    }
                    let mut name = String::new();
                    last.write(&mut name);
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
            }
            if let Some(index) = unknown {
                return match index {
                    0 => first,
                    _ => tied.swap_remove(index - 1),
                };
            }
            if names.len() == 1 {
                return first;
            }
            let others = names[1..].join(", ");
            let reason = format!(
                "cannot be sent with {others}; send only one of {}",
                names.join(", ")
            );
            Failure::new(&first.path, reason, Kind::Unknown)
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
