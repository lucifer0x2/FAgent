//! The SQLite base load and commit step: the full snapshot read under the writer
//! lock, rows from the candidate, then the project leaves memory.
use crate::*;

/// One validated batch, ready to persist.
pub(super) struct Commit<'a> {
    pub before: &'a Snapshot,
    pub candidate: &'a Candidate,
    pub key: &'a str,
    pub hash: &'a str,
    pub history: &'a Value,
    pub result: &'a BatchResult,
}

/// The base revision a batch validates against and its identity.
pub(super) type Base = (Snapshot, Option<SnapshotCacheIdentity>);

impl Project {
    /// Call under the project writer lock.
    pub(super) fn load_base(&self, conn: &mut Connection) -> Result<Base> {
        use backend::sqlite::identity;
        let stamp = identity::file_stamp(self)?;
        let before = bench_phase::time("load", || read_snapshot(conn))?;
        let readiness = before
            .assets
            .iter()
            .map(|asset| self.asset_path(&asset.relative_key).is_ok())
            .collect();
        let meta = identity::snapshot_meta(&before);
        let current = identity::unchanged_identity(self, &meta, stamp, readiness)?;
        Ok((before, current))
    }

    /// Persists the candidate's rows. Commits do not stage in memory yet: a
    /// successful one evicts the project, and the next read reloads it.
    pub(super) fn commit(
        &self,
        conn: &mut Connection,
        c: Commit<'_>,
        metrics: &mut BatchMetrics,
    ) -> Result<()> {
        let _phase = bench_phase::Span::new("persist");
        self.persist(
            conn,
            c.before,
            c.candidate,
            c.key,
            c.hash,
            c.history,
            c.result,
            metrics,
        )?;
        // Still under the writer lock: no read can tag the old revision afterwards
        // without seeing the new identity first.
        self.root.memory.evict(&self.project_id);
        Ok(())
    }
}
