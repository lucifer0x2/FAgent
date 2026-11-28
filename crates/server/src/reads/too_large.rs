//! `CONTEXT_ITEM_TOO_LARGE` errors of paged reads. Each names what did not fit (`error.field`,
//! the item ID when it has one) with its byte size, and suggests only actions that can make it
//! fit: a larger `budget` only while the request is below the maximum.
use super::pagination::json_len;
use crate::error::{ApiError, Result};
use serde_json::{json, Value};

/// The largest `budget` a paged read accepts (`schemas.rs`); list tools always page at it.
pub(super) const MAX_BUDGET: usize = 65_536;
/// A caller may reserve a few bytes of the requested budget (find_nodes renames `nodes` to
/// `matches`); a budget this close to the maximum cannot usefully be raised.
const RESERVED: usize = 16;

/// Page metadata alone exceeds `budget`: names its largest top-level field.
pub(super) fn metadata(response: &Value, bytes: usize, budget: usize) -> Result<ApiError> {
    let mut largest = ("", 0);
    for (key, value) in response.as_object().into_iter().flatten() {
        let size = json_len(value)?;
        if size > largest.1 {
            largest = (key.as_str(), size);
        }
    }
    let (field, size) = largest;
    let mut hints = vec![match field {
        "computed" => "request fewer viewports",
        "criteria" => "send fewer ids or shorter criteria",
        "diagnostics" if response.get("sceneKey").is_some() => "lower diagnosticsLimit",
        _ => "read a smaller frame or node scope",
    }
    .to_owned()];
    hints.extend(raise(budget));
    let message = format!(
        "Context metadata exceeds the response budget: it is {bytes} bytes against a budget of {budget}, and its largest field `{field}` is {size} bytes; {}",
        hints.join(", or ")
    );
    Ok(error(&message, field, bytes, budget))
}

/// One complete entry does not fit on a page of its own: names its field, ID and size.
/// `metadata` is the size of the page without the entry.
pub(super) fn item(
    response: &Value,
    field: &str,
    value: &Value,
    metadata: usize,
    budget: usize,
) -> Result<ApiError> {
    let bytes = json_len(value)?;
    // Row-format nodes are arrays whose first column is the ID.
    let id = ["id", "rootId"]
        .iter()
        .find_map(|key| value.get(*key))
        .or_else(|| value.get(0))
        .and_then(Value::as_str);
    let what = match id {
        Some(id) => format!("`{field}` item `{id}`"),
        None => format!("a `{field}` item"),
    };
    let mut hints = Vec::new();
    match (field, id) {
        ("nodes", _) if response.get("sceneKey").is_some() => {
            hints.push("request fields:[\"bounds\"] to drop text lines".to_owned())
        }
        ("nodes", _) => hints.push("pass properties to keep fewer keys per node".to_owned()),
        ("components", Some(id)) => hints.push(format!(
            "read that master with get_subtree {{nodeId:\"{id}\"}}, or pass instances:\"expand\" to page its nodes one by one"
        )),
        _ => {}
    }
    hints.extend(raise(budget));
    if hints.is_empty() {
        hints.push("this item has no smaller form".to_owned());
    }
    let message = format!(
        "One complete item exceeds the response budget: {what} is {bytes} bytes, and the {budget}-byte budget also holds {metadata} bytes of page metadata; {}",
        hints.join(", or ")
    );
    let mut error = error(&message, field, bytes, budget);
    if let Some(id) = id {
        error = error.with_detail("itemId", json!(id));
    }
    Ok(error)
}

/// find_nodes pages its entries as `nodes` and returns them as `matches`: name that field.
pub(super) fn as_matches(mut error: ApiError) -> ApiError {
    if error.0.code == "CONTEXT_ITEM_TOO_LARGE" && error.0.field.as_deref() == Some("nodes") {
        error.0.field = Some("matches".into());
        error.0.message = error.0.message.replace("`nodes` item", "`matches` item");
    }
    error
}

fn raise(budget: usize) -> Option<String> {
    (budget.saturating_add(RESERVED) < MAX_BUDGET)
        .then(|| format!("raise budget (at most {MAX_BUDGET})"))
}

fn error(message: &str, field: &str, bytes: usize, budget: usize) -> ApiError {
    let mut error = ApiError::new("CONTEXT_ITEM_TOO_LARGE", message);
    error.0.field = Some(field.to_owned());
    error
        .with_detail("bytes", json!(bytes))
        .with_detail("budget", json!(budget))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(error: &ApiError) -> &str {
        &error.0.message
    }

    #[test]
    fn a_larger_budget_is_suggested_only_below_the_maximum() {
        let response = json!({"nodes":[],"components":[]});
        let item = json!({"rootId":"master","nodes":[{"id":"master"}]});
        let below = super::item(&response, "components", &item, 120, 4096).unwrap();
        assert!(message(&below).contains("raise budget"), "{below:?}");
        assert!(message(&below).contains("get_subtree {nodeId:\"master\"}"));
        assert_eq!(below.0.field.as_deref(), Some("components"));
        assert_eq!(below.value()["error"]["itemId"], "master");
        let bytes = json_len(&item).unwrap();
        assert_eq!(below.value()["error"]["bytes"], bytes);
        assert!(message(&below).contains(&format!("is {bytes} bytes")));
        // find_nodes reserves 2 bytes of the 65,536 maximum for its rename.
        for budget in [MAX_BUDGET, MAX_BUDGET - 2] {
            let at_max = super::item(&response, "components", &item, 120, budget).unwrap();
            assert!(!message(&at_max).contains("raise budget"), "{at_max:?}");
        }
        let token = super::item(&response, "tokens", &json!({"id":"t"}), 1, MAX_BUDGET).unwrap();
        assert!(message(&token).ends_with("this item has no smaller form"));
    }

    #[test]
    fn metadata_error_names_its_largest_field() {
        let response =
            json!({"nodes":[],"diagnostics":["x".repeat(300)],"computed":{"v":"y".repeat(900)}});
        let error = metadata(&response, 1300, MAX_BUDGET).unwrap();
        assert_eq!(error.0.field.as_deref(), Some("computed"));
        assert!(message(&error).contains("request fewer viewports"));
        assert!(!message(&error).contains("raise budget"));
        let error = metadata(&response, 1300, 1024).unwrap();
        assert!(message(&error).contains("raise budget (at most 65536)"));
    }
}
