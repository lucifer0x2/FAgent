//! Task28 I2 differential: every read through a state with memory reads on must
//! equal, byte for byte, the same read through a state with them off, on the same
//! data. Reports counts and at most the first differing scopes and JSON paths.
mod fixture;
mod pg_large;
mod queries;
mod realdata;
mod tests;
mod timing;

use super::memory_verify::first_difference;
use crate::state::{AppState, Config};
use fagent_storage::{BatchRequest, NodeMemoryShadowStats, Storage};
use serde_json::{json, Value};

/// Two application states over identical data: memory reads on and off.
pub(super) struct Pair {
    pub on: AppState,
    pub off: AppState,
    pub project: String,
    pub compared: usize,
    pub differences: Vec<String>,
    /// One database behind both states: commit through `on` (true) or `off` only.
    pub shared: Option<bool>,
}

pub(super) fn state(storage: Storage) -> AppState {
    let workspace = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut config = Config::new(workspace, 0);
    // Context reads never wait on render workers; viewports reads are not compared.
    config.node = "missing-memory-diff-worker".into();
    AppState::new(storage, config)
}

async fn call(state: &AppState, name: &str, args: &Value) -> Value {
    super::read(state, name, args.clone())
        .await
        .unwrap_or_else(|e| e.value())
}

impl Pair {
    pub fn new(on: Storage, off: Storage, project: &str) -> Self {
        Self {
            on: state(on),
            off: state(off),
            project: project.into(),
            compared: 0,
            differences: Vec::new(),
            shared: None,
        }
    }

    /// One read on both states; returns the memory-on response.
    pub async fn one(&mut self, name: &str, mut args: Value) -> Value {
        args["projectId"] = json!(self.project);
        let on = call(&self.on, name, &args).await;
        let off = call(&self.off, name, &args).await;
        self.compared += 1;
        if serde_json::to_vec(&on).unwrap() != serde_json::to_vec(&off).unwrap() {
            let scope = args["frameId"].as_str().or(args["nodeId"].as_str());
            let path = first_difference(&on, &off, String::new());
            if self.differences.len() < 10 {
                self.differences
                    .push(format!("{name} {} at {path}", scope.unwrap_or("-")));
            }
        }
        on
    }

    /// Follows `nextCursor` (taken from the memory-on response) to the last page.
    pub async fn walk(&mut self, name: &str, mut args: Value) -> Vec<Value> {
        let mut pages = Vec::new();
        for _ in 0..500 {
            let page = self.one(name, args.clone()).await;
            let next = page["nextCursor"].clone();
            let more = page["truncated"] == true && next.is_string();
            pages.push(page);
            if !more {
                return pages;
            }
            args["cursor"] = next;
        }
        panic!("{name}: cursor did not terminate");
    }

    /// The same batch committed through both states (separate stores), or once
    /// through `shared` when both states use one database.
    pub async fn commit(&mut self, revision: u64, key: &str, operations: Value) -> u64 {
        let states = match self.shared {
            None => vec![&self.on, &self.off],
            Some(true) => vec![&self.on],
            Some(false) => vec![&self.off],
        };
        let mut committed = Vec::new();
        for state in states {
            let request: BatchRequest = serde_json::from_value(json!({
                "projectId": self.project, "expectedRevision": revision,
                "idempotencyKey": key, "operations": operations,
            }))
            .unwrap();
            let id = self.project.clone();
            let result = state
                .db(move |store| store.open_project(&id)?.apply_batch(request))
                .await
                .unwrap_or_else(|e| panic!("{key}: {}", e.value()["error"]));
            committed.push(result.committed_revision);
        }
        assert!(committed.windows(2).all(|w| w[0] == w[1]));
        committed[0]
    }

    pub fn assert_identical(&self, label: &str, start: NodeMemoryShadowStats) {
        let end = fagent_storage::node_memory_shadow_stats();
        let summary = json!({
            "compared": self.compared, "differences": self.differences.len(),
            "readHits": end.read_hits - start.read_hits,
            "readFallbacks": end.read_fallbacks - start.read_fallbacks,
            "readLoads": end.read_loads - start.read_loads,
            "readReloads": end.read_reloads - start.read_reloads,
            "readErrors": end.read_errors - start.read_errors,
            "shadowMismatches": end.mismatches - start.mismatches,
        });
        println!("memory-diff {label} {summary}");
        assert!(self.differences.is_empty(), "{:?}", self.differences);
        assert_eq!(end.mismatches, start.mismatches);
        assert!(end.read_hits > start.read_hits, "memory served no read");
    }
}
