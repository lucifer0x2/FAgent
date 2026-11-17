//! The SQLite base load and commit step: today's path (full snapshot read, rows
//! from the candidate, then the shadow mirror) or, with Task28 I3 writes on, the
//! base snapshot from memory and rows staged in it.
use crate::shadow::{memory_rows, MemoryBase};
use crate::*;

/// One validated batch, ready to persist.
pub(super) struct Commit<'a> {
    pub before: &'a Snapshot,
    pub candidate: &'a Candidate,
    pub operations: &'a [Operation],
    pub key: &'a str,
    pub hash: &'a str,
    pub history: &'a Value,
    pub result: &'a BatchResult,
    pub identity: Option<&'a SnapshotCacheIdentity>,
}

/// The base revision a batch validates against, where it came from and its identity.
pub(super) type Base = (Snapshot, Option<SnapshotCacheIdentity>, Option<MemoryBase>);

impl Project {
    /// Call under the project writer lock. Memory serves the base only for commits,
    /// and only at the identity the database reports now.
    pub(super) fn load_base(&self, conn: &mut Connection, dry_run: bool) -> Result<Base> {
        use backend::sqlite::identity;
        let stamp = identity::file_stamp(self)?;
        let shadow = &self.root.shadow;
        if shadow.writes() && !dry_run {
            if let Some(current) = identity::probe_on(self, conn, stamp.clone())? {
                if let Some((before, memory)) = shadow.memory_base(&current) {
                    return Ok((before, Some(current), Some(memory)));
                }
            }
        }
        let before = bench_phase::time("load", || read_snapshot(conn))?;
        let readiness = before
            .assets
            .iter()
            .map(|asset| self.asset_path(&asset.relative_key).is_ok())
            .collect();
        let meta = identity::snapshot_meta(&before);
        let current = identity::unchanged_identity(self, &meta, stamp, readiness)?;
        Ok((before, current, None))
    }

    pub(super) fn commit(
        &self,
        conn: &mut Connection,
        c: Commit<'_>,
        memory: Option<MemoryBase>,
        metrics: &mut BatchMetrics,
    ) -> Result<()> {
        let _phase = bench_phase::Span::new("persist");
        let Some(memory) = memory else {
            return self.commit_database(conn, &c, metrics);
        };
        let Some(staged) = memory.stage(c.operations) else {
            // The lib rejected what the validator accepted: today's rows, reloaded later.
            self.root.shadow.evict(&self.project_id);
            return self.commit_database(conn, &c, metrics);
        };
        memory.verify(&staged, c.candidate, &c.result.changed_ids);
        let rows = memory_rows(&staged);
        let persisted = self.persist_memory(
            conn, c.before, &rows, c.key, c.hash, c.history, c.result, metrics,
        );
        match persisted {
            // Still under the writer lock: memory moves with the database, in order.
            Ok(()) => drop(staged.commit()),
            Err(error) => {
                // Dropping the staged batch rolls memory back exactly; a commit whose
                // outcome is unknown must not keep the project HOT either.
                drop(staged);
                self.root.shadow.write_aborted();
                self.root.shadow.evict(&self.project_id);
                return Err(error);
            }
        }
        Ok(())
    }

    fn commit_database(
        &self,
        conn: &mut Connection,
        c: &Commit<'_>,
        metrics: &mut BatchMetrics,
    ) -> Result<()> {
        let shadow = &self.root.shadow;
        let mut rows = shadow.enabled().then(crate::shadow::Rows::default);
        self.persist(
            conn,
            c.before,
            c.candidate,
            c.key,
            c.hash,
            c.history,
            c.result,
            metrics,
            rows.as_mut(),
        )?;
        if let Some(rows) = &rows {
            // Still under the project writer lock: commits reach the shadow in order.
            let ready = |snapshot: &mut Snapshot| self.apply_readiness(snapshot);
            shadow.observe(crate::shadow::Commit {
                before: c.before,
                operations: c.operations,
                result: c.result,
                rows,
                identity: c.identity,
                ready: Some(&ready),
            });
        }
        Ok(())
    }
}
