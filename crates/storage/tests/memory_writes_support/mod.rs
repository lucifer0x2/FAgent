//! Writes differential: one project (same id) in two storages over identical data.
//! `warm` keeps its node memory between requests; `cold` has the project evicted
//! before every request, so each batch stages in a fresh load of the stored rows.
//! Every outcome and stored row must be identical, both memories must equal the
//! database snapshot after each commit, and a batch at the current base must give
//! what `Candidate` apply + `validate()` gives.
#![allow(dead_code)]
pub mod gen;
pub mod rows;

use fagent_scene_input::validate::Candidate;
use fagent_storage::*;
use gen::{Ids, Rng};
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// Two storages holding the same project id. `rows(warm)` dumps one side's stored
/// rows as comparable text.
pub struct Pair {
    pub warm: Storage,
    pub cold: Storage,
    pub project: String,
    pub rows: Box<dyn Fn(bool) -> Vec<String>>,
}

#[derive(Debug, Default)]
pub struct Tally {
    pub committed: usize,
    pub rejected: usize,
    pub conflicts: usize,
    pub concurrent: usize,
    pub replays: usize,
    pub previews: usize,
    /// Requests at the current base checked against `Candidate`.
    pub oracle: usize,
}

/// A result or error as the API would serialize it.
fn outcome<T: serde::Serialize>(result: Result<T>) -> Value {
    match result {
        Ok(value) => serde_json::to_value(value).unwrap(),
        Err(error) => json!({"error": error}),
    }
}

fn send(storage: &Storage, project: &str, request: &Value, dry: bool) -> Value {
    let p = storage.open_project(project).unwrap();
    if request.get("pageIds").is_some() {
        let r: ScopedBatchRequest = serde_json::from_value(request.clone()).unwrap();
        return match dry {
            true => outcome(p.dry_run_scoped_batch(r)),
            false => outcome(p.apply_scoped_batch(r)),
        };
    }
    let r: BatchRequest = serde_json::from_value(request.clone()).unwrap();
    match dry {
        true => outcome(p.dry_run_batch(r)),
        false => outcome(p.apply_batch(r)),
    }
}

/// `Candidate` apply per operation, link pruning and a full `validate()`: the
/// changed ids, or the error the batch must fail with.
fn reference(before: &Snapshot, operations: &Value) -> std::result::Result<Vec<String>, Value> {
    let operations: Vec<Operation> = serde_json::from_value(operations.clone()).unwrap();
    let mut candidate = Candidate::from_snapshot(before);
    let mut changed = BTreeSet::new();
    for (index, op) in operations.iter().enumerate() {
        if let Err(error) = candidate.apply(op, &mut changed) {
            return Err(serde_json::to_value(error.at(index)).unwrap());
        }
    }
    candidate.prune_links(&mut changed);
    match candidate.validate() {
        Ok(()) => Ok(changed.into_iter().collect()),
        Err(error) => Err(serde_json::to_value(error).unwrap()),
    }
}

/// Node order aside, the published memory copy equals the database snapshot.
pub fn assert_memory_matches_database(storage: &Storage, project: &str, at: &str) {
    let sorted = |mut snapshot: Snapshot| {
        snapshot.nodes.sort_by(|a, b| a.id.cmp(&b.id));
        snapshot
    };
    let memory = storage
        .memory_snapshot(project)
        .unwrap_or_else(|| panic!("{at}: not resident after a commit"));
    let database = storage.snapshot(project).unwrap();
    assert_eq!(sorted(memory), sorted(database), "{at}: memory differs");
}

impl Pair {
    pub fn seed(&self) {
        let request = json!({"projectId":self.project,"expectedRevision":0,"idempotencyKey":"seed","operations":gen::seed()});
        self.both(&request, false);
    }

    /// Sends `request` to the warm store, then to the cold one after evicting its
    /// memory, and asserts identical outcomes; after a commit both memories hold the
    /// database's state.
    pub fn both(&self, request: &Value, dry: bool) -> Value {
        let warm = send(&self.warm, &self.project, request, dry);
        self.cold.evict_memory(&self.project);
        let cold = send(&self.cold, &self.project, request, dry);
        let key = &request["idempotencyKey"];
        assert_eq!(warm, cold, "outcomes differ for {key}");
        if warm.get("committedRevision").is_some() && warm["replayed"] != true {
            for storage in [&self.warm, &self.cold] {
                assert_memory_matches_database(storage, &self.project, &key.to_string());
            }
        }
        warm
    }

    /// `both`, and for a request at the current base the outcome `Candidate` gives.
    fn checked(&self, request: &Value, dry: bool, tally: &mut Tally) -> Value {
        let before = self.cold.snapshot(&self.project).unwrap();
        let base = request["expectedRevision"].as_u64();
        let guarded = request["strict"] == true && request.get("pageIds").is_some();
        let current = base.is_none_or(|base| base == before.revision) && !guarded;
        let result = self.both(request, dry);
        if !current {
            return result;
        }
        tally.oracle += 1;
        let key = &request["idempotencyKey"];
        match reference(&before, &request["operations"]) {
            Ok(changed) => {
                assert!(
                    result.get("error").is_none(),
                    "{key}: only storage rejects: {result}"
                );
                assert_eq!(result["changedIds"], json!(changed), "{key}: changed ids");
            }
            Err(error) => assert_eq!(result["error"], error, "{key}: error differs"),
        }
        result
    }

    pub fn assert_rows(&self, step: usize) {
        let (warm, cold) = ((self.rows)(true), (self.rows)(false));
        if warm != cold {
            let first: Vec<_> = warm
                .iter()
                .zip(&cold)
                .filter(|(a, b)| a != b)
                .take(2)
                .collect();
            panic!(
                "rows differ after step {step} ({} vs {}): {first:?}",
                warm.len(),
                cold.len()
            );
        }
    }

    /// `steps` batches from three agents that reread only sometimes, so most bases
    /// are stale; some strict, some scoped, some replays and previews.
    pub fn run(&self, seed: u64, steps: usize) -> Tally {
        let mut rng = Rng::new(seed);
        let mut ids = Ids::default();
        let mut bases = [1u64; 3];
        let mut current = 1u64;
        let mut sent: Vec<Value> = vec![];
        let mut tally = Tally::default();
        for step in 0..steps {
            let agent = rng.below(3);
            if rng.chance(30) {
                bases[agent] = current;
            }
            if !sent.is_empty() && rng.chance(5) {
                let again = sent[rng.below(sent.len())].clone();
                self.both(&again, false);
                tally.replays += 1;
                continue;
            }
            ids.refresh(&self.cold.snapshot(&self.project).unwrap());
            let count = 1 + rng.below(3);
            let operations: Vec<Value> = (0..count)
                .map(|_| gen::operation(&mut rng, &mut ids))
                .collect();
            let mut request = json!({"projectId":self.project,"idempotencyKey":format!("s{seed}-{step}"),"operations":operations});
            if !rng.chance(10) {
                request["expectedRevision"] = json!(bases[agent]);
                if rng.chance(25) {
                    request["strict"] = json!(true);
                }
            }
            if rng.chance(15) {
                request["pageIds"] = json!(["pg"]);
            }
            let dry = rng.chance(5);
            let result = self.checked(&request, dry, &mut tally);
            if dry {
                tally.previews += 1;
                continue;
            }
            sent.push(request);
            match result.get("error") {
                Some(error) if error["code"] == "REVISION_CONFLICT" => tally.conflicts += 1,
                Some(_) => tally.rejected += 1,
                None => {
                    tally.committed += 1;
                    current = result["committedRevision"].as_u64().unwrap();
                    tally.concurrent += usize::from(result.get("concurrent").is_some());
                    if rng.chance(50) {
                        bases[agent] = current;
                    }
                }
            }
            if step % 25 == 24 {
                self.assert_rows(step);
            }
        }
        self.assert_rows(steps);
        tally
    }
}
