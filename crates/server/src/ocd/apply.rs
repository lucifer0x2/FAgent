//! `apply_batch` with `source`: the compiled operations replace `source`, followed by any JSON
//! `operations` sent with it, and the combined batch is checked like an inline one. Errors
//! about a compiled operation (the published schema, the batch byte cap, storage errors with
//! `opIndex`) gain the `line` and `column` of the statement that produced it and keep their
//! own code and fields; a storage error on a JSON operation gets its `opIndex` within
//! `operations`. A successful result lists generated node IDs as `autoIds`.
use super::{compile, Compiled, OcdError, MAX_OPERATIONS};
use crate::error::ApiError;
use fagent_storage::{suggest_prop, BatchRequest, MAX_BATCH_BYTES};
use serde_json::{json, Map, Value};

/// Statement positions of the compiled operations, by operation index, the node each one
/// creates or updates, and the node IDs the compiler generated, by line.
#[derive(Debug)]
pub struct Positions {
    lines: Vec<(usize, usize)>,
    nodes: Vec<Option<String>>,
    /// The override targets each compiled `update_node` writes (empty for other operations).
    overrides: Vec<Vec<String>>,
    auto_ids: Vec<(usize, String)>,
}

impl Positions {
    /// Adds the statement position to an error that names a compiled operation with
    /// `opIndex`, or rebases `opIndex` onto `operations` for a JSON operation after them.
    /// An error checked on the whole batch names only its `nodeId`: the last statement that
    /// created or updated that node gives the position.
    pub fn locate(&self, mut error: ApiError) -> ApiError {
        let index = match error.0.op_index {
            Some(index) if index < self.lines.len() => index,
            Some(index) => {
                error.0.op_index = Some(index - self.lines.len());
                return error;
            }
            None => {
                let node = error.0.node.as_ref().and_then(|n| n.node_id.as_deref());
                let index =
                    node.and_then(|id| self.nodes.iter().rposition(|n| n.as_deref() == Some(id)));
                match index {
                    Some(index) => index,
                    None => return error,
                }
            }
        };
        let error = self.not_instance(index, error);
        self.at(index, error)
    }
    /// `@set x ph="A"` or `ph.fill=` on a node that is not an instance: storage rejects the
    /// `overrides` it compiles to; say that descendant keys need an instance, and suggest the
    /// prop a misspelled bare key most likely meant.
    fn not_instance(&self, index: usize, mut error: ApiError) -> ApiError {
        let Some(targets) = self.overrides.get(index).filter(|t| !t.is_empty()) else {
            return error;
        };
        let kind = error.0.node.as_ref().and_then(|n| n.node_kind.as_deref());
        let on_node = matches!(kind, Some(kind) if kind != "instance");
        if error.0.field.as_deref() != Some("props.overrides") || !on_node {
            return error;
        }
        let near = targets.iter().find_map(|t| suggest_prop(t).map(|s| (t, s)));
        let hint = match near {
            Some((target, prop)) => {
                format!("; `{target}` is no node property (did you mean `{prop}`?)")
            }
            None => String::new(),
        };
        error.0.message = format!(
            "{}. Descendant keys (<descendantId>=\"text\", <descendantId>.<key>=…, overrides) need an instance{hint}",
            error.0.message
        );
        if let (Some((_, prop)), Some(context)) = (near, error.0.node.as_mut()) {
            context.suggestion.get_or_insert_with(|| prop.into());
        }
        error
    }
    fn at(&self, index: usize, error: ApiError) -> ApiError {
        match self.lines.get(index) {
            Some(&(line, column)) => error
                .with_detail("line", json!(line))
                .with_detail("column", json!(column)),
            None => error,
        }
    }
    /// Adds `autoIds` (`{"<line>": "<id>"}`) to a batch result when the source generated any.
    pub fn report(&self, result: &mut Value) {
        if self.auto_ids.is_empty() || !result.is_object() {
            return;
        }
        let ids: Map<String, Value> = self
            .auto_ids
            .iter()
            .map(|(line, id)| (line.to_string(), json!(id)))
            .collect();
        result["autoIds"] = Value::Object(ids);
    }
}

/// Compiles `source` into `args["operations"]`, ahead of any JSON operations already there,
/// and validates the request. `pages` is the request's `pageIds`, which count toward the batch
/// byte cap.
pub fn compile_into(
    args: &mut Value,
    source: &str,
    pages: Option<&Value>,
) -> Result<Positions, ApiError> {
    let Compiled {
        mut operations,
        positions,
        auto_ids,
    } = compile(source)?;
    let nodes = operations.iter().map(node_of).collect();
    let overrides = operations.iter().map(override_targets).collect();
    let positions = Positions {
        lines: positions,
        nodes,
        overrides,
        auto_ids,
    };
    if let Some(Value::Array(inline)) = args.get("operations") {
        let total = operations.len() + inline.len();
        if total > MAX_OPERATIONS {
            return Err(ApiError::invalid(&format!(
                "source compiles to {} operations; with {} in operations the batch has {total}, over {MAX_OPERATIONS}: split it into batches",
                operations.len(),
                inline.len()
            )));
        }
        operations.extend(inline.iter().cloned());
    }
    args["operations"] = Value::Array(operations);
    if let Err(error) = crate::schemas::validate("apply_batch", args) {
        let operations = args["operations"].as_array().map_or(&[][..], Vec::as_slice);
        return Err(match crate::schemas::invalid_operation(operations) {
            Some(index) => positions.at(index, error),
            None => error,
        });
    }
    fits(args, pages, &positions)?;
    Ok(positions)
}

/// Storage caps the canonical request (typed operations plus `pageIds`) at 2 MiB; a source
/// that compiles past it fails here, at the statement whose operation crosses the cap. A cap
/// crossed by a JSON operation fails in storage as for an inline batch.
fn fits(args: &Value, pages: Option<&Value>, positions: &Positions) -> Result<(), OcdError> {
    // A request storage cannot decode fails later with its usual error.
    let Ok(request) = serde_json::from_value::<BatchRequest>(args.clone()) else {
        return Ok(());
    };
    let pages = pages.map_or(0, |p| r#","pageIds":"#.len() + p.to_string().len());
    let total = json_len(&request) + pages;
    if total <= MAX_BATCH_BYTES {
        return Ok(());
    }
    // Each operation plus its comma; the rest is the request envelope.
    let sizes: Vec<usize> = request
        .operations
        .iter()
        .map(|op| json_len(op) + 1)
        .collect();
    let mut used = total.saturating_sub(sizes.iter().sum());
    let index = sizes
        .iter()
        .position(|bytes| {
            used += bytes;
            used > MAX_BATCH_BYTES
        })
        .unwrap_or(sizes.len().saturating_sub(1));
    let Some(&(line, column)) = positions.lines.get(index) else {
        return Ok(());
    };
    Err(OcdError::new(
        line,
        column,
        format!("Source compiles to a {total}-byte batch, over the {MAX_BATCH_BYTES}-byte (2 MiB) cap from this statement on; split it into batches"),
    ))
}

/// The node a `create_node` or `update_node` writes.
fn node_of(op: &Value) -> Option<String> {
    match op["op"].as_str() {
        Some("create_node" | "update_node") => op["id"].as_str().map(Into::into),
        _ => None,
    }
}

/// The descendant IDs an `update_node` overrides.
fn override_targets(op: &Value) -> Vec<String> {
    match (op["op"].as_str(), op["props"]["overrides"].as_object()) {
        (Some("update_node"), Some(targets)) => targets.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

fn json_len<T: serde::Serialize>(value: &T) -> usize {
    serde_json::to_vec(value).map_or(0, |bytes| bytes.len())
}
