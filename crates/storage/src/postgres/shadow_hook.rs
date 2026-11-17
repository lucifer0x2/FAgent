//! Task28 I1: hands PostgreSQL commits to the node-memory shadow on the caller,
//! after the worker reported the commit, like the write-through cache fill.
use super::*;
use crate::{BatchResult, Operation};

impl PostgresBackend {
    /// The operations to mirror, copied only while the shadow is on.
    pub(super) fn shadow_operations(&self, operations: &[Operation]) -> Option<Vec<Operation>> {
        self.shadow.enabled().then(|| operations.to_vec())
    }

    /// Mirrors a new commit; a replay carries no rows and is not mirrored.
    pub(super) fn observe(
        &self,
        operations: Option<Vec<Operation>>,
        result: &BatchResult,
        committed: Option<&write_through::Committed>,
    ) {
        if let (Some(operations), Some(committed)) = (operations, committed) {
            committed.observe(&self.shadow, &operations, result);
        }
    }
}

impl PostgresBackend {
    /// Today's shared snapshot with the identity of the same read (Task28 I2).
    pub(super) fn shared_with_identity(&self, project: &str) -> Result<crate::shadow::Loaded> {
        use crate::StorageBackend;
        let (snapshot, identity) = self.snapshot_with_cache_identity(project)?;
        let indexes = self.indexes.for_snapshot(&snapshot, identity.clone());
        Ok((
            crate::SharedSnapshot::committed(snapshot, indexes),
            identity,
        ))
    }
}
