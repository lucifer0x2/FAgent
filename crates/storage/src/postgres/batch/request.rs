//! Request checks and history reads shared by commits and previews.
use super::super::*;
use crate::{BatchRequest, StorageError, MAX_BATCH_BYTES, MAX_BATCH_OPS};
use serde_json::Value;

/// Request checks shared by commits and previews: the request's project and
/// key, the operation count and the canonical size. Returns the request value
/// and its canonical hash.
pub(super) fn canonical(
    project: &projects::ProjectRow,
    request: &BatchRequest,
    pages: Option<&[String]>,
) -> Result<(Value, String)> {
    transaction::request_ids(
        &project.meta.project_id,
        &request.project_id,
        &request.idempotency_key,
    )?;
    if request.operations.is_empty() || request.operations.len() > MAX_BATCH_OPS {
        return Err(StorageError::validation(
            "operations",
            "Batch requires 1–500 operations",
        ));
    }
    let value = crate::scoped::request_value(request, pages)?;
    let bytes = serde_json::to_vec(&crate::canonical_json(&value))?;
    if bytes.len() > MAX_BATCH_BYTES {
        return Err(StorageError::validation(
            "operations",
            "Batch exceeds 2 MiB",
        ));
    }
    Ok((value, crate::hash(&bytes)))
}

/// At most 1,001 commits after `base`, `Value::Null` past 8 MiB of encoded history.
pub(in crate::postgres) fn history_after<C: ::postgres::GenericClient>(
    db: &mut Db<'_, C>,
    project: &str,
    base: u64,
) -> Result<Vec<(u64, Value)>> {
    let _phase = crate::bench_phase::Span::new("history.after");
    let rows = db.query("SELECT revision,CASE WHEN sum(octet_length(operations)) OVER (ORDER BY revision)<=8388608 THEN operations ELSE 'null' END FROM commits WHERE project_id=$1 AND revision>$2 ORDER BY revision LIMIT 1001",
        &[&project, &(base as i64)])?;
    rows.into_iter()
        .map(|row| {
            Ok((
                row.get::<_, i64>(0) as u64,
                serde_json::from_str(&row.get::<_, String>(1))?,
            ))
        })
        .collect()
}
