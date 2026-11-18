//! One PostgreSQL commit as the adapter runs it: the batch staged in node memory
//! and persisted on a pool worker, then the snapshot cache entry is evicted.
use super::*;
use crate::{BatchRequest, BatchResult};

impl PostgresBackend {
    pub(super) fn commit(
        &self,
        project: &str,
        request: BatchRequest,
        pages: Option<Vec<String>>,
    ) -> Result<BatchResult> {
        let key = project.to_owned();
        let memory = self.memory.clone();
        let (result, committed) = self.pool.run(move |session| {
            match batch::apply_mode(session, &key, request, false, pages, &memory)? {
                batch::Outcome::Commit(result, committed) => Ok((result, committed)),
                _ => unreachable!("commit operation"),
            }
        })?;
        self.evict_cached(committed);
        Ok(result)
    }

    /// A preview stages the batch in memory and rolls it back.
    pub(super) fn preview(
        &self,
        project: &str,
        request: BatchRequest,
        pages: Option<Vec<String>>,
    ) -> Result<crate::DryRunResult> {
        let key = project.to_owned();
        let memory = self.memory.clone();
        self.pool.run(move |session| {
            match batch::apply_mode(session, &key, request, true, pages, &memory)? {
                batch::Outcome::Preview(result) => Ok(result),
                _ => unreachable!("preview operation"),
            }
        })
    }
}
