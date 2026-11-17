//! Task28 I3 differential: one project (same id) in two storages, writes through
//! memory off and on; every result and every stored row must be identical.
#![allow(dead_code)]
pub mod gen;
pub mod rows;

use fagent_storage::*;
use gen::{Ids, Rng};
use serde_json::{json, Value};

/// Two storages holding the same project id: `off` commits today's way, `on`
/// through memory. `rows` dumps each one's stored rows as comparable text.
pub struct Pair {
    pub off: Storage,
    pub on: Storage,
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

impl Pair {
    pub fn seed(&self) {
        let request = json!({"projectId":self.project,"expectedRevision":0,"idempotencyKey":"seed","operations":gen::seed()});
        self.both(&request, false);
    }

    /// Sends `request` to both storages and asserts identical outcomes.
    pub fn both(&self, request: &Value, dry: bool) -> Value {
        let off = send(&self.off, &self.project, request, dry);
        let on = send(&self.on, &self.project, request, dry);
        assert_eq!(off, on, "outcomes differ for {}", request["idempotencyKey"]);
        off
    }

    pub fn assert_rows(&self, step: usize) {
        let (off, on) = ((self.rows)(false), (self.rows)(true));
        if off != on {
            let first: Vec<_> = off
                .iter()
                .zip(&on)
                .filter(|(a, b)| a != b)
                .take(2)
                .collect();
            panic!(
                "rows differ after step {step} ({} vs {}): {first:?}",
                off.len(),
                on.len()
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
            ids.refresh(&self.off.snapshot(&self.project).unwrap());
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
            let result = self.both(&request, dry);
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
