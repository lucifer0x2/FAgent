//! Committed and previewed batches: request checks here, staging in `NodeMemory`
//! (`memory`) under the project writer lock.
use crate::*;
use serde::Serialize;
mod history;
mod memory;
/// The candidate path batches used before `NodeMemory`, kept as the test oracle
/// for `memory::guard` and the scoped parity tests until phase 4.
#[cfg(test)]
mod validation;
#[cfg(test)]
pub(crate) use validation::validated_candidate;

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
        let checked = memory::Checked {
            request: &request,
            value: &request_value,
            hash: &request_hash,
            base,
            pages: pages.as_deref(),
            dry_run,
        };
        let outcome = self.stage(&mut conn, checked, &mut metrics)?;
        metrics.total_us = us(total);
        Ok((outcome, metrics))
    }
}
