//! Runtime enforcement for the exact JSON Schema vocabulary emitted by this server.
//!
//! A rejection names the first failing field as a path (`idempotencyKey`,
//! `operations[3].props`) in the message and in `error.field`, plus `opIndex` for a batch
//! operation, so a caller can fix the one value instead of guessing at the whole call.
//! Which fields are accepted does not depend on this reporting.
use crate::error::Result;
use serde_json::{Map, Value};
mod failure;
use failure::{blame, list, Failure, Kind, Step};

pub fn validate(schema: &Value, value: &Value) -> Result<()> {
    check(schema, value, &mut Vec::new()).map_err(Failure::into_error)
}

type Checked = std::result::Result<(), Failure>;

fn check(schema: &Value, value: &Value, path: &mut Vec<Step>) -> Checked {
    if let Some(choices) = schema["oneOf"].as_array() {
        one_of(choices, value, path)?;
    }
    if let Some(expected) = schema.get("const") {
        if expected != value {
            let kind = Kind::Discriminator {
                expected: expected.clone(),
                missing: false,
            };
            return Err(Failure::new(path, format!("must be {expected}"), kind));
        }
    }
    if let Some(choices) = schema["enum"].as_array() {
        if !choices.contains(value) {
            let reason = format!("must be one of {}", list(choices));
            return Err(Failure::value(path, reason));
        }
    }
    if let Some(types) = schema.get("type") {
        check_type(types, value, path)?;
    }
    if let Some(n) = value.as_f64() {
        check_number(schema, n, path)?;
    }
    if let Some(s) = value.as_str() {
        check_string(schema, s, path)?;
    }
    if let Some(array) = value.as_array() {
        check_array(schema, array, path)?;
    }
    if let Some(object) = value.as_object() {
        check_object(schema, object, path)?;
    }
    Ok(())
}

/// Exactly one variant must match. With none, report the variant that came closest.
fn one_of(choices: &[Value], value: &Value, path: &mut Vec<Step>) -> Checked {
    let mut matched = 0;
    let mut failures = Vec::new();
    for choice in choices {
        match check(choice, value, path) {
            Ok(()) => matched += 1,
            Err(failure) => failures.push((choice, failure)),
        }
    }
    match matched {
        1 => Ok(()),
        0 => Err(blame(path, value, failures)),
        _ => Err(Failure::value(path, "matches more than one accepted form")),
    }
}

fn check_type(types: &Value, value: &Value, path: &[Step]) -> Checked {
    let is = |t: &str| match t {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        _ => false,
    };
    let names: Vec<String> = match types {
        Value::Array(names) => names
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        other => other.as_str().map(str::to_owned).into_iter().collect(),
    };
    if names.iter().any(|t| is(t)) {
        return Ok(());
    }
    let reason = format!("must be of type {}", names.join(" or "));
    Err(Failure::new(path, reason, Kind::Type(names)))
}

fn check_number(schema: &Value, n: f64, path: &[Step]) -> Checked {
    if !n.is_finite() {
        return Err(Failure::value(path, "must be a finite number"));
    }
    if let Some(min) = schema["minimum"].as_f64().filter(|min| n < *min) {
        return Err(Failure::value(path, format!("must be at least {min}")));
    }
    if let Some(max) = schema["maximum"].as_f64().filter(|max| n > *max) {
        return Err(Failure::value(path, format!("must be at most {max}")));
    }
    if let Some(min) = schema["exclusiveMinimum"].as_f64().filter(|min| n <= *min) {
        return Err(Failure::value(path, format!("must be greater than {min}")));
    }
    Ok(())
}

fn check_string(schema: &Value, s: &str, path: &[Step]) -> Checked {
    let n = s.chars().count() as u64;
    if let Some(min) = schema["minLength"].as_u64().filter(|min| n < *min) {
        let reason = format!("must have at least {min} characters");
        return Err(Failure::value(path, reason));
    }
    if let Some(max) = schema["maxLength"].as_u64().filter(|max| n > *max) {
        let reason = format!("must have at most {max} characters");
        return Err(Failure::value(path, reason));
    }
    if let Some(pattern) = schema["pattern"].as_str() {
        if !matches_pattern(pattern, s) {
            return Err(Failure::value(path, format!("must match {pattern}")));
        }
    }
    Ok(())
}

/// The patterns this server publishes, checked without a regex engine.
fn matches_pattern(pattern: &str, s: &str) -> bool {
    match pattern {
        "^[A-Za-z0-9_/-]+$" => {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_/-".contains(&b))
        }
        "^[A-Za-z0-9_-]+$" => {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        }
        "^[a-z0-9]+(-[a-z0-9]+)*$" => s.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        }),
        _ => false,
    }
}

fn check_array(schema: &Value, array: &[Value], path: &mut Vec<Step>) -> Checked {
    let n = array.len() as u64;
    if let Some(min) = schema["minItems"].as_u64().filter(|min| n < *min) {
        return Err(Failure::value(
            path,
            format!("must have at least {min} items"),
        ));
    }
    if let Some(max) = schema["maxItems"].as_u64().filter(|max| n > *max) {
        return Err(Failure::value(
            path,
            format!("must have at most {max} items"),
        ));
    }
    if let Some(items) = schema.get("items") {
        for (i, item) in array.iter().enumerate() {
            nested(path, Step::Index(i), |path| check(items, item, path))?;
        }
    }
    Ok(())
}

fn check_object(schema: &Value, object: &Map<String, Value>, path: &mut Vec<Step>) -> Checked {
    if let Some(max) = schema["maxProperties"]
        .as_u64()
        .filter(|max| object.len() as u64 > *max)
    {
        return Err(Failure::value(
            path,
            format!("must have at most {max} fields"),
        ));
    }
    let properties = schema["properties"].as_object();
    let required: Vec<&str> = schema["required"]
        .as_array()
        .map(|keys| keys.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    // Discriminators first, so a variant of the wrong kind says so and never outranks the
    // variant of the right kind that failed further in.
    let discriminators = properties
        .into_iter()
        .flatten()
        .filter_map(|(key, field)| field.get("const").map(|c| (key, field, c)));
    for (key, field, expected) in discriminators {
        let step = Step::Key(key.clone());
        match object.get(key) {
            Some(item) => nested(path, step, |path| check(field, item, path))?,
            None if required.contains(&key.as_str()) => {
                let at = [path.as_slice(), &[step]].concat();
                let kind = Kind::Discriminator {
                    expected: expected.clone(),
                    missing: true,
                };
                return Err(Failure::new(&at, "is required", kind));
            }
            None => {}
        }
    }
    // Unknown fields before missing ones: a field sent to the wrong variant is the clearer
    // report when two variants each take a different source.
    let known = |key: &String| properties.is_some_and(|p| p.contains_key(key));
    if schema["additionalProperties"] == false {
        if let Some(key) = object.keys().find(|key| !known(key)) {
            let at = [path.as_slice(), &[Step::Key(key.clone())]].concat();
            return Err(Failure::new(&at, "is not an accepted field", Kind::Unknown));
        }
    }
    let missing: Vec<String> = required
        .iter()
        .filter(|key| !object.contains_key(**key))
        .map(|key| (*key).to_owned())
        .collect();
    if let Some(first) = missing.first() {
        let at = [path.as_slice(), &[Step::Key(first.clone())]].concat();
        let reason = match missing.len() {
            1 => "is required".to_owned(),
            _ => format!("is required (missing {})", missing.join(", ")),
        };
        return Err(Failure::new(&at, reason, Kind::Required(missing)));
    }
    for (key, item) in object {
        let step = Step::Key(key.clone());
        if let Some(field) = properties.and_then(|p| p.get(key)) {
            nested(path, step, |path| check(field, item, path))?;
        } else if schema["additionalProperties"].is_object() {
            nested(path, step, |path| {
                check(&schema["additionalProperties"], item, path)
            })?;
        }
    }
    Ok(())
}

fn nested(path: &mut Vec<Step>, step: Step, f: impl FnOnce(&mut Vec<Step>) -> Checked) -> Checked {
    path.push(step);
    let result = f(path);
    path.pop();
    result
}
