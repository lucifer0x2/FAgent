//! Commits after a base revision, for page guards and `concurrent` reports.
use crate::*;

/// At most 1,001 commits after `base`; once their encoded history passes 8 MiB the
/// rest come back as `Value::Null` (callers treat that as incomplete history).
pub(crate) fn after(conn: &Connection, base: u64) -> Result<Vec<(u64, Value)>> {
    let _phase = bench_phase::Span::new("history.after");
    let mut stmt = conn.prepare_cached("SELECT revision,CASE WHEN sum(length(CAST(ops_json AS BLOB))) OVER (ORDER BY revision)<=8388608 THEN ops_json ELSE 'null' END FROM commits WHERE revision>?1 ORDER BY revision LIMIT 1001")?;
    let rows = stmt.query_map([base as i64], |r| {
        Ok((r.get::<_, i64>(0)? as u64, r.get::<_, String>(1)?))
    })?;
    rows.map(|row| {
        let (revision, text) = row?;
        Ok((revision, serde_json::from_str(&text)?))
    })
    .collect()
}
