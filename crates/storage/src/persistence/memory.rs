//! Task28 I3: persists rows staged in memory. Like the candidate path, a row whose
//! value equals the base row is not rewritten, so the stored text is identical.
use super::{begin, finish};
use crate::shadow::MemoryRows;
use crate::*;
use std::collections::{HashMap, HashSet};

/// The base rows of `ids`, by one pass over a base table.
fn base_rows<'a, T>(
    rows: &'a [T],
    ids: impl Iterator<Item = &'a str>,
    id: impl Fn(&T) -> &str,
) -> HashMap<&'a str, &'a T> {
    let wanted: HashSet<&str> = ids.collect();
    rows.iter()
        .filter(|row| wanted.contains(id(row)))
        .map(|row| (id(row), row))
        .collect()
}

impl Project {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn persist_memory(
        &self,
        conn: &mut Connection,
        before: &Snapshot,
        rows: &MemoryRows,
        key: &str,
        request_hash: &str,
        ops: &Value,
        result: &BatchResult,
        metrics: &mut BatchMetrics,
    ) -> Result<()> {
        let time = Instant::now();
        let tx = begin(conn, before.revision)?;
        for id in &rows.deleted_nodes {
            tx.execute("DELETE FROM nodes WHERE id=?1", [id])?;
        }
        let old = base_rows(
            &before.nodes,
            rows.nodes.iter().map(|n| n.id.as_str()),
            |n| &n.id,
        );
        {
            let mut stmt=tx.prepare_cached("INSERT INTO nodes(id,parent_id,sort_key,kind,props_json) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET parent_id=excluded.parent_id,sort_key=excluded.sort_key,kind=excluded.kind,props_json=excluded.props_json")?;
            for n in rows
                .nodes
                .iter()
                .filter(|n| old.get(n.id.as_str()) != Some(n))
            {
                stmt.execute(params![
                    n.id,
                    n.parent_id,
                    n.sort_key,
                    n.kind.as_str(),
                    serde_json::to_string(&n.props)?
                ])?;
            }
        }
        let old = base_rows(
            &before.tokens,
            rows.tokens.iter().map(|t| t.id.as_str()),
            |t| &t.id,
        );
        for t in rows
            .tokens
            .iter()
            .filter(|t| old.get(t.id.as_str()) != Some(t))
        {
            tx.execute("INSERT INTO tokens(id,data_json) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET data_json=excluded.data_json",params![t.id,serde_json::to_string(t)?])?;
        }
        let old = base_rows(
            &before.styles,
            rows.styles.iter().map(|s| s.id.as_str()),
            |s| &s.id,
        );
        for s in rows
            .styles
            .iter()
            .filter(|s| old.get(s.id.as_str()) != Some(s))
        {
            tx.execute("INSERT INTO styles(id,data_json) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET data_json=excluded.data_json",params![s.id,serde_json::to_string(s)?])?;
        }
        for id in &rows.deleted_notes {
            tx.execute("DELETE FROM notes WHERE id=?1", [id])?;
        }
        let old = base_rows(
            &before.notes,
            rows.notes.iter().map(|n| n.id.as_str()),
            |n| &n.id,
        );
        for n in rows
            .notes
            .iter()
            .filter(|n| old.get(n.id.as_str()) != Some(n))
        {
            tx.execute("INSERT INTO notes(id,data_json) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET data_json=excluded.data_json",params![n.id,serde_json::to_string(n)?])?;
        }
        finish(
            tx,
            before.revision,
            key,
            request_hash,
            ops,
            result,
            metrics,
            time,
        )
    }
}
