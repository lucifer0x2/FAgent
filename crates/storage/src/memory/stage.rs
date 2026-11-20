//! The steps the SQLite and PostgreSQL commit paths share once the backend holds its
//! write serialization and has the resident project: begin the batch (reloading once
//! when the resident copy is stale), then build the result and the history envelope.
use super::{guard, Memory};
use crate::{bench_phase, scoped, BatchRequest, BatchResult, Result, StorageError};
use fagent_node_memory::{Begin, HotProject, Operation, Prepared};
use serde_json::Value;

impl Memory {
    /// Begins `ops` on `first` at `revision`. A stale copy is a lost race with a
    /// reader's reload under the write serialization: the project is evicted, loaded
    /// again with `reload` into `slot`, and the batch begins there once; stale again is
    /// a revision conflict. Returns the project the batch staged in and the batch.
    pub(crate) fn begin<'p>(
        &self,
        project_id: &str,
        (first, slot): (&'p HotProject, &'p mut Option<HotProject>),
        (revision, ops): (u64, &[Operation]),
        reload: impl FnOnce() -> Result<HotProject>,
    ) -> Result<(&'p HotProject, Prepared<'p>)> {
        if let Begin::Ready(prepared) = begin(first, revision, ops)? {
            return Ok((first, prepared));
        }
        self.evict(project_id);
        let retry: &'p HotProject = slot.insert(reload()?);
        match begin(retry, revision, ops)? {
            Begin::Ready(prepared) => Ok((retry, prepared)),
            Begin::Stale => Err(StorageError::conflict(revision)),
        }
    }
}

/// Begins `ops` on `project` under the `memory.begin` bench phase.
pub(crate) fn begin<'p>(
    project: &'p HotProject,
    revision: u64,
    ops: &[Operation],
) -> Result<Begin<'p>> {
    bench_phase::time("memory.begin", || project.begin(revision, ops))
}

/// The commit's result (with the pairs it overwrote, see [`guard::result`]) and its
/// history envelope. `value` is the canonical request; `history` the commits after
/// the request's base.
pub(crate) fn outcome(
    prepared: &Prepared<'_>,
    request: &BatchRequest,
    history: &[(u64, Value)],
    value: &Value,
) -> Result<(BatchResult, Value)> {
    let operations = &value["operations"];
    let result = guard::result(prepared, request, history, operations);
    let envelope = bench_phase::time("history.envelope", || {
        scoped::envelope_with_scopes(
            prepared.footprint().clone(),
            value.clone(),
            operations.clone(),
        )
    })?;
    Ok((result, envelope))
}
