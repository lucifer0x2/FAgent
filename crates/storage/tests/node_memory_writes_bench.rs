//! Task28 I3 commit latency on PostgreSQL, writes through memory off vs on: 30
//! scoped commits (median) on a generated 200k project and, when given, the largest
//! project of a disposable backup restore. Prints counts and timings only.
//!
//! `scripts/task19-check.sh cargo test --release -p fagent-storage \
//!   --features large-project-bench --test node_memory_writes_bench -- --ignored --nocapture`
//! Optional `WRITES_BENCH_REALDATA_URL` (a restore on another port, never 2703).
#![cfg(feature = "large-project-bench")]
use fagent_storage::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

const COMMITS: usize = 30;

fn generate(nodes: usize) -> Snapshot {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("writes-{nodes}.json"));
    let status = Command::new("node")
        .arg(manifest.join("../node-memory/fixtures/generate.mjs"))
        .args(["--nodes", &nodes.to_string(), "--seed", "7", "--out"])
        .arg(&out)
        .stdout(std::process::Stdio::null())
        .status()
        .expect("node on PATH");
    assert!(status.success());
    serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap()
}

fn seed(url: &str, project: &str, s: &Snapshot) {
    let mut client = postgres::Client::connect(url, postgres::NoTls).unwrap();
    let mut tx = client.transaction().unwrap();
    let insert = tx
        .prepare(
            "INSERT INTO fagent_runtime_v1.entities(project_id,kind,id,data) VALUES($1,$2,$3,$4)",
        )
        .unwrap();
    let text = |v: &dyn erased::Row| v.text();
    let rows = s
        .nodes
        .iter()
        .map(|n| ("node", &n.id, text(n)))
        .chain(s.tokens.iter().map(|t| ("token", &t.id, text(t))))
        .chain(s.styles.iter().map(|t| ("style", &t.id, text(t))))
        .chain(s.fonts.iter().map(|t| ("font", &t.id, text(t))))
        .chain(s.notes.iter().map(|t| ("note", &t.id, text(t))));
    for (kind, id, data) in rows {
        tx.execute(&insert, &[&project, &kind, id, &data]).unwrap();
    }
    tx.commit().unwrap();
}

mod erased {
    pub trait Row {
        fn text(&self) -> String;
    }
    impl<T: serde::Serialize> Row for T {
        fn text(&self) -> String {
            serde_json::to_string(self).unwrap()
        }
    }
}

fn storage(url: &str, writes: WritesMode) -> Storage {
    let mut config = PostgresConfig::new(url);
    config.node_memory_shadow = ShadowMode::Off;
    config.node_memory_reads = ReadsMode::Off;
    config.node_memory_writes = writes;
    let backend = PostgresBackend::with_config(config).unwrap();
    let cache = SnapshotCache::new(backend.cache_namespace());
    Storage::from_shared(Arc::new(backend.with_cache(cache).unwrap()))
}

/// A non-instance child of a root frame, and its page.
fn target(s: &Snapshot) -> (String, String) {
    let parent: BTreeMap<&str, Option<&str>> = s
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n.parent_id.as_deref()))
        .collect();
    let up = |id: &str| parent.get(id).copied().flatten();
    let node = s.nodes.iter().find(|n| {
        n.kind.as_str() != "instance"
            && up(&n.id)
                .and_then(up)
                .is_some_and(|page| up(page).is_none())
    });
    let node = node.expect("a child of a root frame");
    let page = up(&node.id).and_then(up).unwrap();
    (node.id.clone(), page.to_owned())
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    values[values.len() / 2]
}

/// Median wall time and per-phase milliseconds of `COMMITS` scoped commits.
fn measure(storage: &Storage, project: &str, node: &str, page: &str) -> Value {
    let p = storage.open_project(project).unwrap();
    let mut revision = p.snapshot().unwrap().revision;
    let (mut totals, mut phases): (Vec<f64>, BTreeMap<&str, Vec<f64>>) = Default::default();
    for i in 0..COMMITS + 2 {
        let request: ScopedBatchRequest = serde_json::from_value(json!({"projectId":project,
            "expectedRevision":revision,"idempotencyKey":format!("bench-{}", uuid::Uuid::new_v4().simple()),
            "pageIds":[page],"operations":[{"op":"update_node","id":node,"props":{"name":format!("w{i}")}}]})).unwrap();
        bench_phase::take();
        let start = Instant::now();
        revision = p.apply_scoped_batch(request).unwrap().committed_revision;
        let total = start.elapsed().as_secs_f64() * 1e3;
        let taken = bench_phase::take();
        if i < 2 {
            continue; // warm-up: first load into memory and caches
        }
        totals.push(total);
        let mut sums: BTreeMap<&str, f64> = BTreeMap::new();
        for (name, micros) in taken {
            *sums.entry(name).or_default() += micros as f64 / 1e3;
        }
        for (name, ms) in sums {
            phases.entry(name).or_default().push(ms);
        }
    }
    let phases: BTreeMap<&str, f64> = phases
        .into_iter()
        .map(|(name, values)| (name, (median(values) * 100.0).round() / 100.0))
        .collect();
    json!({"totalMs": (median(totals) * 100.0).round() / 100.0, "phasesMs": phases})
}

fn compare(label: &str, url: &str, project: &str, snapshot: &Snapshot) {
    let (node, page) = target(snapshot);
    for (mode, writes) in [("off", WritesMode::Off), ("on", WritesMode::On)] {
        let report = measure(&storage(url, writes), project, &node, &page);
        println!(
            "writes-bench {label} nodes={} {mode} {report}",
            snapshot.nodes.len()
        );
    }
}

#[test]
#[ignore = "Task28 I3 timing on a disposable PostgreSQL (scripts/task19-check.sh)"]
fn commit_latency_off_vs_on() {
    let url = std::env::var("TASK19_POSTGRES_URL").expect("TASK19_POSTGRES_URL");
    assert!(!url.contains(":2703"), "never the live cluster");
    let nodes = std::env::var("WRITES_BENCH_NODES").map_or(200_000, |v| v.parse().unwrap());
    let snapshot = generate(nodes);
    let setup = storage(&url, WritesMode::Off);
    let id = setup
        .create_project("gen-writes", "Generated")
        .unwrap()
        .id()
        .to_owned();
    seed(&url, &id, &snapshot);
    compare("generated", &url, &id, &snapshot);
    if let Ok(real) = std::env::var("WRITES_BENCH_REALDATA_URL") {
        assert!(!real.contains("2703"), "never the live cluster");
        let storage = storage(&real, WritesMode::Off);
        let largest = storage
            .list_projects()
            .unwrap()
            .into_iter()
            .map(|meta| {
                storage
                    .open_project(&meta.project_id)
                    .unwrap()
                    .snapshot()
                    .unwrap()
            })
            .max_by_key(|s| s.nodes.len())
            .unwrap();
        compare("real", &real, &largest.project_id, &largest);
    }
}
