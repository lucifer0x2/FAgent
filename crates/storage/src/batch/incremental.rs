//! SQLite HOT path: stage, validate the delta, persist touched rows, then publish.
use crate::*;

impl Project {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn try_memory_batch(
        &self,
        conn: &mut Connection,
        request: &BatchRequest,
        pages: Option<&[String]>,
        value: &Value,
        hash: &str,
        base: concurrent::Base,
        metrics: &mut BatchMetrics,
    ) -> Result<Option<BatchResult>> {
        let shadow = &self.root.shadow;
        if !shadow.writes() {
            return Ok(None);
        }
        use backend::sqlite::identity;
        let time = Instant::now();
        let Some(mut identity) = identity::probe_on(self, conn, identity::file_stamp(self)?)?
        else {
            return Ok(None);
        };
        let Some(memory) = shadow.memory_handle(&identity) else {
            return Ok(None);
        };
        base.check(identity.revision, pages.is_some())?;
        metrics.read_us = us(time);
        let time = Instant::now();
        let Some((staged, local)) = memory.prepare(&request.operations)? else {
            return Ok(None);
        };
        let history = match base.history_after(identity.revision, pages.is_some()) {
            Some(after) => super::history::after(conn, after)?,
            None => vec![],
        };
        if let (Some(pages), true) = (pages, base.strict) {
            local.check_pages(request, pages, &history)?;
        }
        metrics.validate_us = us(time);
        let result = local.result(request, &history, &value["operations"]);
        let envelope = scoped::envelope_with_scopes(
            local.footprint,
            value.clone(),
            value["operations"].clone(),
        )?;
        let persisted = bench_phase::time("persist", || {
            self.persist_memory(
                conn,
                &local.preimages,
                &local.rows,
                &request.idempotency_key,
                hash,
                &envelope,
                &result,
                metrics,
            )
        });
        match persisted {
            Ok(()) => {
                staged.commit();
            }
            Err(error) => {
                drop(staged);
                shadow.write_aborted();
                shadow.evict(&self.project_id);
                return Err(error);
            }
        }
        identity.revision = result.committed_revision;
        if let Ok(Some(stamp)) = identity::file_stamp(self) {
            identity.incarnation = stamp;
        }
        shadow.retag(&identity);
        if shadow.claim_cache_cleanup(&self.project_id) {
            self.root.indexes.evict(&self.project_id);
        }
        Ok(Some(result))
    }
}
