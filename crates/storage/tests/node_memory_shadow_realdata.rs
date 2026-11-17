//! Task28 I1 real-data check: scripted batches built from real node ids, committed
//! through the PostgreSQL backend with the node-memory shadow on, against a
//! disposable restore of a native backup. Prints counts, timings and at most the
//! ids of the first mismatches; never project content.
//!
//! `SHADOW_REALDATA_URL='host=<socket dir> port=<port> dbname=<db>' cargo test --release \
//!   -p fagent-storage --test node_memory_shadow_realdata -- --ignored --nocapture`
//! Optional: `SHADOW_REALDATA_BATCHES` per project per pass (default 120).
mod shadow_realdata;

use fagent_storage::*;
use serde_json::json;
use shadow_realdata::ops::{batch, Rng, KINDS};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

fn storage(url: &str, mode: ShadowMode) -> Storage {
    let mut config = PostgresConfig::new(url);
    config.node_memory_shadow = mode;
    Storage::from_shared(Arc::new(PostgresBackend::with_config(config).unwrap()))
}

fn median(mut v: Vec<u64>) -> u64 {
    v.sort_unstable();
    v.get(v.len() / 2).copied().unwrap_or(0)
}

/// Runs `count` batches on one project; returns per-commit shadow µs (loads excluded)
/// and the outcome counts by batch label.
fn pass(
    storage: &Storage,
    project: &str,
    count: usize,
    seed: u64,
) -> (Vec<u64>, BTreeMap<String, u64>) {
    let p = storage.open_project(project).unwrap();
    let mut rng = Rng(seed);
    let (mut shadow, mut outcomes) = (Vec::new(), BTreeMap::<String, u64>::new());
    let mut snapshot = p.snapshot().unwrap();
    for step in 0..count {
        let serial = (seed as usize % 1000) * 100_000 + step;
        let Some(b) = batch(&snapshot, step % KINDS, serial, &mut rng) else {
            *outcomes.entry("unavailable".into()).or_default() += 1;
            continue;
        };
        let key = format!("shadow-rd-{seed}-{step}");
        let before = node_memory_shadow_stats();
        let result = match b.pages {
            None => p.apply_batch(BatchRequest {
                project_id: p.id().into(),
                expected_revision: Some(snapshot.revision),
                strict: false,
                idempotency_key: key,
                operations: b.ops,
            }),
            Some(page_ids) => p.apply_scoped_batch(ScopedBatchRequest {
                project_id: p.id().into(),
                expected_revision: Some(snapshot.revision),
                strict: false,
                idempotency_key: key,
                page_ids,
                operations: b.ops,
            }),
        };
        let after = node_memory_shadow_stats();
        let label = match &result {
            Ok(_) => b.label.to_string(),
            Err(e) => format!("{}:rejected:{}", b.label, e.code),
        };
        *outcomes.entry(label).or_default() += 1;
        if result.is_ok() {
            let load = after.load_micros - before.load_micros;
            if load == 0 {
                shadow.push(after.observe_micros - before.observe_micros);
            }
            snapshot = p.snapshot().unwrap();
        }
    }
    (shadow, outcomes)
}

#[test]
#[ignore = "real-data check against a disposable restore; see the module docs"]
fn shadow_matches_real_projects() {
    let url = std::env::var("SHADOW_REALDATA_URL").expect("SHADOW_REALDATA_URL");
    assert!(!url.contains("2703"), "never the live cluster");
    let count = std::env::var("SHADOW_REALDATA_BATCHES").map_or(120, |v| v.parse().unwrap());
    let start = node_memory_shadow_stats();
    // Strict: the whole in-memory project is compared after every commit.
    let strict = storage(&url, ShadowMode::Strict);
    let projects = strict.list_projects().unwrap();
    let mut report = Vec::new();
    for (i, meta) in projects.iter().enumerate() {
        let started = Instant::now();
        let nodes = strict.snapshot(&meta.project_id).unwrap().nodes.len();
        let (_, strict_outcomes) = pass(&strict, &meta.project_id, count, 11 + i as u64);
        // On: the production mode, for per-commit overhead.
        let on = storage(&url, ShadowMode::On);
        let (shadow, on_outcomes) = pass(&on, &meta.project_id, count, 101 + i as u64);
        report.push(json!({
            "project": i, "authoredNodes": nodes, "strict": strict_outcomes, "on": on_outcomes,
            "medianShadowUs": median(shadow.clone()), "maxShadowUs": shadow.iter().max(),
            "seconds": started.elapsed().as_secs(),
        }));
    }
    let end = node_memory_shadow_stats();
    let totals = json!({
        "matched": end.matched - start.matched, "mismatches": end.mismatches - start.mismatches,
        "errors": end.errors - start.errors, "loads": end.loads - start.loads,
        "resyncs": end.resyncs - start.resyncs, "skipped": end.skipped - start.skipped,
        "loadMicros": end.load_micros - start.load_micros,
    });
    println!(
        "shadow-realdata {}",
        json!({"projects": report, "totals": totals})
    );
    assert_eq!(
        (end.mismatches, end.errors),
        (start.mismatches, start.errors)
    );
}
