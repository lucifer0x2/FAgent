//! Persists the rows of a batch staged in memory. `Rows` lists only rows that
//! differ from the base revision, so the stored text of every other row is
//! untouched, as on the candidate path.
use super::{begin, finish};
use crate::*;
use fagent_node_memory::Rows;

impl Project {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn persist_memory(
        &self,
        conn: &mut Connection,
        base: u64,
        rows: &Rows,
        key: &str,
        request_hash: &str,
        ops: &Value,
        result: &BatchResult,
        metrics: &mut BatchMetrics,
    ) -> Result<()> {
        let time = Instant::now();
        let tx = begin(conn, base)?;
        for id in &rows.deleted_nodes {
            tx.execute("DELETE FROM nodes WHERE id=?1", [id])?;
        }
        {
            let mut stmt=tx.prepare_cached("INSERT INTO nodes(id,parent_id,sort_key,kind,props_json) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET parent_id=excluded.parent_id,sort_key=excluded.sort_key,kind=excluded.kind,props_json=excluded.props_json")?;
            for n in &rows.nodes {
                stmt.execute(params![
                    n.id,
                    n.parent_id,
                    n.sort_key,
                    n.kind.as_str(),
                    serde_json::to_string(&n.props)?
                ])?;
            }
        }
        for t in &rows.tokens {
            tx.execute("INSERT INTO tokens(id,data_json) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET data_json=excluded.data_json",params![t.id,serde_json::to_string(t)?])?;
        }
        for s in &rows.styles {
            tx.execute("INSERT INTO styles(id,data_json) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET data_json=excluded.data_json",params![s.id,serde_json::to_string(s)?])?;
        }
        for id in &rows.deleted_notes {
            tx.execute("DELETE FROM notes WHERE id=?1", [id])?;
        }
        for n in &rows.notes {
            tx.execute("INSERT INTO notes(id,data_json) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET data_json=excluded.data_json",params![n.id,serde_json::to_string(n)?])?;
        }
        finish(tx, base, key, request_hash, ops, result, metrics, time)
    }
}
