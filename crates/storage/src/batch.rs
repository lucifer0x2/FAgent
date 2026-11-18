//! One candidate-validation pipeline for committed and previewed batches.
use crate::*;
use serde::Serialize;
mod commit;
mod history;
mod validation;
#[cfg(test)]
pub(crate) use validation::validated_candidate;
pub(crate) use validation::validated_candidate_indexed;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DryRunResult {
    pub project_id: String,
    pub dry_run: bool,
    pub base_revision: u64,
    pub changed_ids: Vec<String>,
    pub diagnostics: Vec<Value>,
}

enum Outcome {
    Committed(BatchResult),
    Preview(DryRunResult),
}

impl Project {
    pub fn apply_batch(&self, request: BatchRequest) -> Result<BatchResult> {
        self.apply_batch_measured(request).map(|(r, _)| r)
    }
    pub fn apply_batch_measured(
        &self,
        request: BatchRequest,
    ) -> Result<(BatchResult, BatchMetrics)> {
        match self.run_batch(request, false, None)? {
            (Outcome::Committed(result), metrics) => Ok((result, metrics)),
            _ => unreachable!("commit mode returns a commit"),
        }
    }
    /// Preview checks existing key collisions without reserving a key. Revision and
    /// candidate validation still run under the same project writer lock.
    pub fn dry_run_batch(&self, request: BatchRequest) -> Result<DryRunResult> {
        match self.run_batch(request, true, None)? {
            (Outcome::Preview(result), _) => Ok(result),
            _ => unreachable!("preview mode returns a preview"),
        }
    }
    pub fn apply_scoped_batch(&self, request: ScopedBatchRequest) -> Result<BatchResult> {
        let (request, pages) = request.split();
        match self.run_batch(request, false, Some(pages))? {
            (Outcome::Committed(result), _) => Ok(result),
            _ => unreachable!(),
        }
    }
    pub fn dry_run_scoped_batch(&self, request: ScopedBatchRequest) -> Result<DryRunResult> {
        let (request, pages) = request.split();
        match self.run_batch(request, true, Some(pages))? {
            (Outcome::Preview(result), _) => Ok(result),
            _ => unreachable!(),
        }
    }
    fn run_batch(
        &self,
        request: BatchRequest,
        dry_run: bool,
        pages: Option<Vec<String>>,
    ) -> Result<(Outcome, BatchMetrics)> {
        let total = Instant::now();
        self.check_paths()?;
        self.request_ids(&request.project_id, &request.idempotency_key)?;
        if request.operations.is_empty() || request.operations.len() > MAX_BATCH_OPS {
            return Err(StorageError::validation(
                "operations",
                "Batch requires 1–500 operations",
            ));
        }
        let request_value = scoped::request_value(&request, pages.as_deref())?;
        let bytes = serde_json::to_vec(&canonical_json(&request_value))?;
        if bytes.len() > MAX_BATCH_BYTES {
            return Err(StorageError::validation(
                "operations",
                "Batch exceeds 2 MiB",
            ));
        }
        let request_hash = hash(&bytes);
        let base = concurrent::Base::of(&request)?;
        let wait = Instant::now();
        let mut conn = self.writer.lock().map_err(|_| poisoned())?;
        let mut metrics = BatchMetrics {
            queue_wait_us: us(wait),
            request_bytes: bytes.len(),
            ..Default::default()
        };
        if let Some(mut result) = replay(&conn, &request.idempotency_key, &request_hash)? {
            if !dry_run {
                result.replayed = true;
                metrics.total_us = us(total);
                return Ok((Outcome::Committed(result), metrics));
            }
        }
        let time = Instant::now();
        let (before, identity) = self.load_base(&mut conn)?;
        let indexes = self.root.indexes.for_snapshot(&before, identity.clone());
        metrics.read_us = us(time);
        base.check(before.revision, pages.is_some())?;
        let time = Instant::now();
        let (candidate, changed_ids, mut work, next_indexes) =
            validated_candidate_indexed(&before, &request.operations, indexes.clone())?;
        let scope = scoped::BatchScope::indexed(
            &before,
            &candidate,
            &changed_ids,
            indexes,
            next_indexes.clone(),
        )?;
        let history = match base.history_after(before.revision, pages.is_some()) {
            Some(after) => history::after(&conn, after)?,
            None => vec![],
        };
        if let (Some(pages), true) = (&pages, base.strict) {
            let _phase = bench_phase::Span::new("scoped.check");
            scoped::check(
                &before, &candidate, &request, pages, &history, &scope, &mut work,
            )?;
        }
        drop(work);
        metrics.validate_us = us(time);
        if dry_run {
            metrics.total_us = us(total);
            return Ok((
                Outcome::Preview(DryRunResult {
                    project_id: self.project_id.clone(),
                    dry_run: true,
                    base_revision: before.revision,
                    changed_ids,
                    diagnostics: vec![],
                }),
                metrics,
            ));
        }
        let mut result = BatchResult {
            project_id: self.project_id.clone(),
            committed_revision: before.revision + 1,
            changed_ids,
            render_status: "pending".into(),
            ..Default::default()
        };
        let operations = &request_value["operations"];
        concurrent::fill(
            &mut result,
            base,
            before.revision,
            &history,
            operations,
            &candidate,
        );
        let history = bench_phase::time("history.envelope", || {
            scoped::envelope_with_scopes(
                scope.footprint,
                request_value.clone(),
                request_value["operations"].clone(),
            )
        })?;
        let commit = commit::Commit {
            before: &before,
            candidate: &candidate,
            key: &request.idempotency_key,
            hash: &request_hash,
            history: &history,
            result: &result,
        };
        self.commit(&mut conn, commit, &mut metrics)?;
        if let Some(mut identity) = identity {
            identity.revision = result.committed_revision;
            // The write may have changed the file stamp; we still hold the writer lock.
            if let Ok(Some(stamp)) = backend::sqlite::identity::file_stamp(self) {
                identity.incarnation = stamp;
            }
            self.root.indexes.publish(identity, next_indexes);
        }
        metrics.total_us = us(total);
        Ok((Outcome::Committed(result), metrics))
    }
}
