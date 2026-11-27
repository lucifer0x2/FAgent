//! The SQLite commit and preview, staged in `NodeMemory` under the writer lock:
//! identity probe, the resident project (or a cold load), the engine's validated
//! batch, page guard and history, then the rows it changed are persisted and the
//! batch is published.
use super::{history, DryRunResult, Outcome};
use crate::memory::{guard, stage};
use crate::*;
use backend::sqlite::identity;
use fagent_node_memory::HotProject;

/// One request after its request-level checks.
pub(super) struct Checked<'r> {
    pub request: &'r BatchRequest,
    /// The canonical request value, with `pageIds` when scoped.
    pub value: &'r Value,
    pub hash: &'r str,
    pub base: concurrent::Base,
    pub pages: Option<&'r [String]>,
    pub dry_run: bool,
}

impl Project {
    /// Call under the project writer lock, after the idempotency replay.
    pub(super) fn stage(
        &self,
        conn: &mut Connection,
        c: Checked<'_>,
        metrics: &mut BatchMetrics,
    ) -> Result<Outcome> {
        let memory = &self.root.memory;
        let time = Instant::now();
        let stamp = identity::file_stamp(self)?;
        let (revision, current) = identity::probe_on(self, conn, stamp)?;
        c.base.check(revision, c.pages.is_some())?;
        let first = self.for_write(conn, current.as_ref(), revision)?;
        metrics.read_us = us(time);
        let time = Instant::now();
        let mut retry = None;
        let (project, prepared) = memory.begin(
            &self.project_id,
            (&first, &mut retry),
            (revision, &c.request.operations),
            || self.for_write(conn, current.as_ref(), revision),
        )?;
        let history = match c.base.history_after(revision, c.pages.is_some()) {
            Some(after) => history::after(conn, after)?,
            None => vec![],
        };
        if let (Some(pages), true) = (c.pages, c.base.strict) {
            let _phase = bench_phase::Span::new("scoped.check");
            guard::check(&prepared, c.request, pages, &history)?;
        }
        metrics.validate_us = us(time);
        if c.dry_run {
            return Ok(Outcome::Preview(DryRunResult {
                project_id: self.project_id.clone(),
                dry_run: true,
                base_revision: revision,
                changed_ids: prepared.changed_ids().to_vec(),
                deleted_note_ids: prepared.deleted_note_ids().to_vec(),
                diagnostics: vec![],
            }));
        }
        let (result, envelope) = stage::outcome(&prepared, c.request, &history, c.value)?;
        let rows = bench_phase::time("persist.rows", || prepared.rows());
        let persisted = {
            let _phase = bench_phase::Span::new("persist");
            let key = &c.request.idempotency_key;
            let (envelope, result) = (&envelope, &result);
            self.persist_memory(
                conn, revision, &rows, key, c.hash, envelope, result, metrics,
            )
        };
        if let Err(error) = persisted {
            drop(prepared);
            memory.write_failed(&self.project_id);
            return Err(error);
        }
        prepared.commit();
        // Still under the writer lock: the write may have changed the file stamp.
        let next = current.map(|mut identity| {
            identity.revision = result.committed_revision;
            if let Ok(Some(stamp)) = identity::file_stamp(self) {
                identity.incarnation = stamp;
            }
            identity
        });
        memory.committed(project, next.as_ref());
        Ok(Outcome::Committed(result))
    }

    /// The resident project at `revision`, or a cold load of the stored rows with
    /// asset readiness, as reads see them. A load tagged with `current` takes the asset
    /// readiness `current` observed, so the snapshot and its tag agree even when a file
    /// appears or disappears in between.
    fn for_write(
        &self,
        conn: &mut Connection,
        current: Option<&SnapshotCacheIdentity>,
        revision: u64,
    ) -> Result<HotProject> {
        let load = || {
            let mut snapshot = bench_phase::time("load", || read_snapshot(conn))?;
            match current {
                Some(identity) => {
                    self.apply_readiness_from(&mut snapshot, &identity.asset_readiness)
                }
                None => self.apply_readiness(&mut snapshot),
            }
            Ok(snapshot)
        };
        let memory = &self.root.memory;
        memory.for_write(&self.project_id, current, revision, load)
    }
}
