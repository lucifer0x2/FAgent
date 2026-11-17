//! HOT batch preparation from an exact staged delta and immutable revision views.
use super::{MemoryBase, MemoryRows};
use crate::{
    concurrent, validate, BatchRequest, BatchResult, Operation, Result, Snapshot,
    SnapshotCacheIdentity,
};
use fagent_node_memory::Staged;
use fagent_scene_input::read_scope::ReadView;
use std::{collections::BTreeSet, sync::Arc};

#[cfg(test)]
#[path = "incremental_tests.rs"]
mod tests;

pub(crate) struct LocalBatch {
    pub before: Arc<ReadView>,
    pub after: Arc<ReadView>,
    /// Exact touched preimages, not a complete snapshot. Persistence only compares
    /// these IDs; never hand this to the reference validator or full scope index.
    pub preimages: Snapshot,
    pub rows: MemoryRows,
    pub changed: Vec<String>,
    pub footprint: BTreeSet<String>,
}

impl MemoryBase {
    pub fn view(&self) -> Arc<ReadView> {
        self.handle.read_view()
    }

    pub fn prepare<'a>(
        &'a self,
        operations: &[Operation],
    ) -> Result<Option<(Staged<'a>, LocalBatch)>> {
        let before = self.view();
        let result = self.prepare_local(operations);
        // Feature strict mode is a deliberate test oracle, distinct from a
        // request's strict revision guard. Production on-mode stays local.
        if self.strict && !matches!(result, Ok(None)) {
            let mut reference = validate::Candidate::from_snapshot(&before.to_snapshot());
            let mut changed = BTreeSet::new();
            let expected = (|| -> Result<()> {
                for (i, op) in operations.iter().enumerate() {
                    reference.apply(op, &mut changed).map_err(|e| e.at(i))?;
                }
                reference.prune_links(&mut changed);
                reference.validate()
            })();
            let public = |error: Option<&crate::StorageError>| {
                error.map(|e| serde_json::to_value(e).unwrap())
            };
            assert_eq!(
                public(result.as_ref().err()),
                public(expected.as_ref().err()),
                "memory delta/reference validation error"
            );
            if let Ok(Some((staged, local))) = &result {
                let ids: Vec<_> = changed.into_iter().collect();
                assert_eq!(local.changed, ids, "memory delta/reference changed ids");
                self.verify(staged, &reference, &ids);
            }
        }
        result
    }

    fn prepare_local<'a>(
        &'a self,
        operations: &[Operation],
    ) -> Result<Option<(Staged<'a>, LocalBatch)>> {
        let before = self.view();
        if !validate::memory::supports(&before, operations) {
            return Ok(None);
        }
        // A lib rejection takes the reference path to preserve error precedence
        // against earlier property errors and public structural error contexts.
        let Some(mut staged) = self.stage(operations) else {
            return Ok(None);
        };
        let after = staged.preview();
        let changed = staged.delta().changed_ids();
        let Some(work) = crate::bench_phase::time("memory.validate.delta", || {
            validate::memory::validate_delta(&before, &after, operations, &changed)
        })?
        else {
            return Ok(None);
        };
        crate::bench_phase::count("validation.authored", work.stats.authored_nodes);
        crate::bench_phase::count("validation.affected", work.stats.affected_nodes);
        crate::bench_phase::count("validation.expanded", work.stats.expanded_nodes);
        drop(work);
        super::stats::bump(&super::stats::COUNTERS.write_hits);
        super::stats::bump(&super::stats::COUNTERS.write_incremental);
        let rows = super::memory_rows(&staged);
        let mut preimages = before.metadata().as_ref().clone();
        preimages.nodes = changed
            .iter()
            .filter_map(|id| before.node(id).cloned())
            .collect();
        preimages.notes = changed
            .iter()
            .filter_map(|id| before.note(id).cloned())
            .collect();
        let mut footprint = BTreeSet::new();
        for id in &changed {
            if serde_json::to_vec(&before.node(id))? != serde_json::to_vec(&after.node(id))? {
                footprint.insert(format!("node:{id}"));
                for view in [&before, &after] {
                    if let Some(page) = view.page(id) {
                        footprint.insert(format!("page:{page}"));
                    }
                }
            }
            if before.note(id) != after.note(id) {
                footprint.insert(format!("note:{id}"));
                for view in [&before, &after] {
                    if let Some(page) = view.note(id).and_then(|note| view.page(&note.node_id)) {
                        footprint.insert(format!("page:{page}"));
                    }
                }
            }
        }
        Ok(Some((
            staged,
            LocalBatch {
                before,
                after,
                preimages,
                rows,
                changed,
                footprint,
            },
        )))
    }
}

impl LocalBatch {
    /// Same strict guard and sealed history coverage as scoped::check. The local
    /// classifier excludes global/resource/note/root-order operations; those keep
    /// the full reference guard. Both endpoint dependency closures are required.
    pub fn check_pages(
        &self,
        request: &BatchRequest,
        pages: &[String],
        history: &[(u64, serde_json::Value)],
    ) -> Result<()> {
        use crate::{NodeKind, StorageError};
        let base = request
            .expected_revision
            .unwrap_or(self.before.metadata().revision);
        let revision = self.before.metadata().revision;
        if base > revision || history.len() > 1000 {
            return Err(StorageError::conflict(revision));
        }
        crate::scoped::index::PageIndex::validate_page_ids(pages)?;
        if pages.iter().any(|id| {
            self.before
                .node(id)
                .is_none_or(|n| n.kind != NodeKind::Page)
        }) {
            return Err(StorageError::validation(
                "pageIds",
                "Guarded page does not exist",
            ));
        }
        for op in &request.operations {
            let (id, parent) = match op {
                Operation::CreateNode { id, parent_id, .. } => (id, parent_id.as_deref()),
                Operation::UpdateNode { id, .. }
                | Operation::DeleteSubtree { id }
                | Operation::ReorderNode { id, .. } => (id, None),
                Operation::ReparentNode { id, parent_id, .. } => (id, Some(parent_id.as_str())),
                _ => unreachable!("local operation classifier"),
            };
            for (view, target) in [(&self.before, id.as_str()), (&self.after, id.as_str())]
                .into_iter()
                .chain(parent.map(|p| (&self.after, p)))
            {
                if view
                    .page(target)
                    .is_some_and(|page| !pages.iter().any(|p| p == page))
                {
                    return Err(StorageError::new(
                        "SCOPED_OPERATION_UNSUPPORTED",
                        "Operation source and destination pages must be guarded",
                    ));
                }
            }
        }
        if self
            .footprint
            .iter()
            .filter_map(|key| key.strip_prefix("page:"))
            .any(|page| !pages.iter().any(|p| p == page))
        {
            return Err(StorageError::new("SCOPED_OPERATION_UNSUPPORTED", "Every affected page, including link cleanup, note anchors and source/destination pages, must be guarded"));
        }
        let mut guard = BTreeSet::new();
        if base != revision {
            guard = self.before.dependency_closure(pages)?;
            guard.extend(self.after.dependency_closure(pages)?);
        }
        let mut expected = base + 1;
        for (r, value) in history {
            if *r != expected || value["scopeVersion"] != 1 {
                return Err(StorageError::conflict(revision));
            }
            let coverage =
                crate::scoped::coverage(value, &self.before.metadata().project_id, r - 1)
                    .map_err(|_| StorageError::conflict(revision))?;
            if coverage.iter().any(|key| key == "*" || guard.contains(key)) {
                return Err(StorageError::conflict(revision));
            }
            expected += 1;
        }
        if expected != revision + 1 {
            return Err(StorageError::conflict(revision));
        }
        Ok(())
    }

    pub fn result(
        &self,
        request: &BatchRequest,
        history: &[(u64, serde_json::Value)],
        operations: &serde_json::Value,
    ) -> BatchResult {
        let mut result = BatchResult {
            project_id: self.before.metadata().project_id.clone(),
            committed_revision: self.after.metadata().revision,
            changed_ids: self.changed.clone(),
            render_status: "pending".into(),
            ..Default::default()
        };
        if let Some(expected) = request
            .expected_revision
            .filter(|base| *base < self.before.metadata().revision)
        {
            let ours = concurrent::batch_writes(operations, &self.changed, |id| {
                let meta = self.after.metadata();
                self.after.node(id).is_some()
                    || self.after.note(id).is_some()
                    || meta.tokens.iter().any(|t| t.id == id)
                    || meta.styles.iter().any(|s| s.id == id)
            });
            concurrent::report(
                &mut result,
                expected,
                self.before.metadata().revision,
                history,
                &ours,
            );
        }
        result
    }
}

impl super::Shadow {
    /// No snapshot conversion: identity is probed by the builtin engine under its
    /// writer lock. A certificate is bound to the exact returned immutable view.
    pub fn memory_handle(&self, identity: &SnapshotCacheIdentity) -> Option<MemoryBase> {
        let (Some(strict), Some(reads), Some(mirror)) = (self.writes, &self.reads, &self.mirror)
        else {
            return None;
        };
        mirror.maybe_expire();
        let handle = mirror.registry.get(&identity.project_id)?;
        if !reads.tagged(&handle, identity)
            || super::reads::published_revision(&handle, identity.revision) != identity.revision
        {
            return None;
        }
        Some(MemoryBase { handle, strict })
    }
}
