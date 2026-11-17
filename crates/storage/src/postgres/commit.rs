//! One PostgreSQL commit as the adapter runs it: through memory when Task28 I3
//! writes are on, then the shadow mirror (today's path only) and the cache fill.
use super::*;
use crate::{BatchRequest, BatchResult, Operation};
use write_through::Committed;

type Done = (BatchResult, Option<Box<Committed>>, Option<Vec<Operation>>);

impl PostgresBackend {
    /// A batch the lib rejects after validation runs again on today's path, with
    /// the project evicted; its first attempt wrote nothing.
    pub(super) fn commit(
        &self,
        project: &str,
        request: BatchRequest,
        pages: Option<Vec<String>>,
    ) -> Result<BatchResult> {
        let memory = self.shadow.writes();
        let retry = memory.then(|| (request.clone(), pages.clone()));
        let project_id = request.project_id.clone();
        let (result, committed, operations) =
            match (self.commit_once(project, request, pages, memory), retry) {
                (Err(e), Some((request, pages))) if e.code == memory_write::FALLBACK => {
                    self.shadow.evict(&project_id);
                    self.commit_once(project, request, pages, false)?
                }
                (outcome, _) => outcome?,
            };
        self.observe(operations, &result, committed.as_deref());
        if self.shadow.claim_cache_cleanup(&result.project_id) {
            self.indexes.evict(&result.project_id);
            if let Some(cache) = &self.cache {
                cache.evict(&result.project_id);
            }
        }
        self.fill(committed);
        Ok(result)
    }

    fn commit_once(
        &self,
        project: &str,
        request: BatchRequest,
        pages: Option<Vec<String>>,
        memory: bool,
    ) -> Result<Done> {
        let key = project.to_owned();
        let operations = self.shadow_operations(&request.operations);
        let indexes = self.indexes.clone();
        let shadow = memory.then(|| self.shadow.clone());
        let (result, committed) = self.pool.run(move |session| {
            let outcome = batch::apply_mode(
                session,
                &key,
                request,
                false,
                pages,
                &indexes,
                shadow.as_ref(),
            )?;
            match outcome {
                batch::Outcome::Commit(result, committed) => Ok((result, committed)),
                _ => unreachable!("commit operation"),
            }
        })?;
        Ok((result, committed, operations))
    }
}
