//! One PostgreSQL commit as the adapter runs it: the database path, then the
//! project leaves node memory and the write-through cache is filled.
use super::*;
use crate::{BatchRequest, BatchResult};

impl PostgresBackend {
    /// Commits do not stage in memory yet: a new commit evicts the project inside
    /// the pool job, and the next read reloads it at the new revision.
    pub(super) fn commit(
        &self,
        project: &str,
        request: BatchRequest,
        pages: Option<Vec<String>>,
    ) -> Result<BatchResult> {
        let key = project.to_owned();
        let indexes = self.indexes.clone();
        let memory = self.memory.clone();
        let (result, committed) = self.pool.run(move |session| {
            let memory = Some(&memory);
            match batch::apply_mode(session, &key, request, false, pages, &indexes, memory)? {
                batch::Outcome::Commit(result, committed) => Ok((result, committed)),
                _ => unreachable!("commit operation"),
            }
        })?;
        self.fill(committed);
        Ok(result)
    }
}
