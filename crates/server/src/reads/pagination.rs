use super::ReadRequest;
use crate::error::{ApiError, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use fagent_storage::Snapshot;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// Request fields a cursor does not bind: they shape each entry or the page size, never the
/// entry list the offset indexes. `revision` is bound through the snapshot revision instead.
const UNBOUND: [&str; 5] = ["cursor", "properties", "budget", "limit", "revision"];

/// The cursor scope of a snapshot read: a 16-hex-digit hash of everything bound, followed by the revision
/// and a short digest per bound field (`<hash>.<revision>.<field>:<digest>,…`), so a mismatch
/// can name the changed parameter instead of a generic `INVALID_CURSOR`.
pub fn scope(s: &Snapshot, tool: &str, req: &ReadRequest) -> Result<(String, usize)> {
    if let Some(requested) = req.revision.filter(|r| *r != s.revision) {
        return Err(super::revision_unavailable(requested, s.revision));
    }
    let mut query = serde_json::to_value(req)?;
    let fields = query.as_object_mut().unwrap();
    fields.retain(|key, value| !UNBOUND.contains(&key.as_str()) && !value.is_null());
    let mut digests = vec![format!("tool:{}", digest(&json!(tool))?)];
    for (key, value) in fields.iter() {
        digests.push(format!("{key}:{}", digest(value)?));
    }
    let hash = Sha256::digest(serde_json::to_vec(&json!([
        s.project_id,
        s.revision,
        tool,
        query
    ]))?);
    let hash = format!("{hash:x}");
    let scope = format!("{}.{}.{}", &hash[..16], s.revision, digests.join(","));
    let offset = offset(&scope, req.cursor.as_deref())?;
    Ok((scope, offset))
}
/// First eight hex digits of the SHA-256 of `value`'s JSON.
fn digest(value: &Value) -> Result<String> {
    let hash = format!("{:x}", Sha256::digest(serde_json::to_vec(value)?));
    Ok(hash[..8].to_owned())
}
/// A scope from [`scope`]: its revision and per-field digests, or `None` for another format.
fn parts(scope: &str) -> Option<(u64, Vec<(&str, &str)>)> {
    let mut parts = scope.splitn(3, '.');
    let (_, revision, fields) = (parts.next()?, parts.next()?, parts.next()?);
    let fields = fields
        .split(',')
        .map(|field| field.split_once(':'))
        .collect::<Option<Vec<_>>>()?;
    Some((revision.parse().ok()?, fields))
}
/// Why a cursor from `found` does not continue the read `expected`: a newer revision, or the
/// first bound parameter whose value changed (named in the message and `error.field`).
fn mismatch(expected: &str, found: &str) -> ApiError {
    let (Some((current, want)), Some((issued, got))) = (parts(expected), parts(found)) else {
        return invalid();
    };
    if current != issued {
        let mut error = ApiError::new(
            "INVALID_CURSOR",
            &format!("Cursor belongs to revision {issued}, but the project is now at revision {current}; restart the read without cursor"),
        );
        error.0.current_revision = Some(current);
        error.0.field = Some("cursor".into());
        return error;
    }
    let value = |list: &[(&str, &str)], key: &str| {
        list.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, digest)| digest.to_string())
    };
    let changed = want
        .iter()
        .chain(&got)
        .map(|(key, _)| *key)
        .find(|key| value(&want, key) != value(&got, key));
    let Some(key) = changed else { return invalid() };
    let mut error = ApiError::new(
        "INVALID_CURSOR",
        &format!("Cursor belongs to a read with another {key}; repeat {key} as on the first page, or restart the read without cursor. properties, budget and limit may change between pages"),
    );
    error.0.field = Some(key.into());
    error
}
/// Scope hash and offset for a read whose identity is not a snapshot query, such as an
/// immutable render job: `identity` must hold every parameter the cursor binds.
pub fn scope_of(identity: &Value, cursor: Option<&str>) -> Result<(String, usize)> {
    let scope = format!("{:x}", Sha256::digest(serde_json::to_vec(identity)?));
    let offset = offset(&scope, cursor)?;
    Ok((scope, offset))
}
fn offset(scope: &str, cursor: Option<&str>) -> Result<usize> {
    let Some(raw) = cursor else { return Ok(0) };
    let decoded = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
    let value: Value = serde_json::from_slice(&decoded).map_err(|_| invalid())?;
    if value["scope"] != scope {
        return Err(mismatch(scope, value["scope"].as_str().unwrap_or_default()));
    }
    value["offset"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(invalid)
}
fn invalid() -> ApiError {
    ApiError::new(
        "INVALID_CURSOR",
        "Cursor is malformed, stale or belongs to another read scope; restart the read",
    )
}
fn cursor(scope: &str, offset: usize) -> String {
    URL_SAFE_NO_PAD.encode(json!({"scope":scope,"offset":offset}).to_string())
}

/// Extra metadata can use the space left by the pagination envelope and the first
/// entry on this page. This keeps an optional summary from preventing cursor progress.
pub(super) fn metadata_headroom(
    response: &Value,
    first_entry: Option<&(String, Value)>,
    offset: usize,
    budget: usize,
    scope: &str,
) -> Result<usize> {
    let mut used = json_len(response)? + ",\"nextCursor\":null,\"truncated\":false".len();
    if let Some((field, value)) = first_entry {
        used += json_len(value)? + cursor(scope, offset + 1).len() - 3;
        if response.get(field).is_none() {
            used += json_len(field)? + 4;
        }
    }
    Ok(budget.saturating_sub(used))
}
/// Every entry is consumed once, including resources; a budget never silently drops content.
/// The response is serialized once; each step then adds the exact compact-JSON growth (the
/// entry, its comma, a newly created array key and the cursor length change), so a page costs
/// O(bytes) instead of re-serializing the whole response per entry. Page boundaries and bytes
/// are the same as measuring the full response after every entry (`pagination_tests.rs`).
pub fn page(
    mut response: Value,
    entries: Vec<(String, Value)>,
    offset: usize,
    limit: usize,
    budget: usize,
    scope: &str,
) -> Result<Value> {
    if offset > entries.len() {
        return Err(invalid());
    }
    let mut next = offset;
    response["truncated"] = json!(false);
    response["nextCursor"] = Value::Null;
    let base = json_len(&response)?;
    if base > budget {
        return Err(ApiError::new("CONTEXT_ITEM_TOO_LARGE", "Context metadata exceeds the response budget; request fewer viewports or a larger budget"));
    }
    // While a page is open it is measured as truncated:true with the cursor after the last
    // entry: `false`→`true` is −1 byte and `null`→`"<cursor>"` is +len−2, hence `len − 3`.
    let mut added = 0;
    while let Some((field, value)) = entries.get(next) {
        if next - offset >= limit {
            break;
        }
        if field != "nodes"
            && response[field]
                .as_array()
                .is_some_and(|list| list.len() >= 100)
        {
            break;
        }
        if response.get(field).is_none() {
            response[field] = json!([]);
            added += json_len(field.as_str())? + 4; // `,"field":[]`
        }
        let list = response[field].as_array_mut().unwrap();
        let grow = json_len(value)? + usize::from(!list.is_empty());
        list.push(value.clone());
        if base + added + grow + cursor(scope, next + 1).len() - 3 > budget {
            list.pop();
            if next == offset {
                return Err(ApiError::new("CONTEXT_ITEM_TOO_LARGE", "One complete item exceeds the response budget; use a property filter or larger budget"));
            }
            break;
        }
        added += grow;
        next += 1;
    }
    let truncated = next < entries.len();
    response["truncated"] = json!(truncated);
    response["nextCursor"] = if truncated {
        json!(cursor(scope, next))
    } else {
        Value::Null
    };
    Ok(response)
}
/// Compact JSON length of `value`, counted without building the string.
pub(super) fn json_len<T: Serialize + ?Sized>(value: &T) -> Result<usize> {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count(0);
    serde_json::to_writer(&mut count, value)?;
    Ok(count.0)
}
#[cfg(test)]
#[path = "pagination_tests.rs"]
mod tests;
