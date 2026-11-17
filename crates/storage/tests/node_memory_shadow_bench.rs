//! Task28 I1 overhead: per-commit cost of the node-memory shadow on a generated
//! project (default 200k expanded nodes) committed through SQLite. Prints counts
//! and timings only; the fixture stays under the cargo temp dir. Needs Node.js.
//!
//! `cargo test --release -p fagent-storage --features large-project-bench \
//!   --test node_memory_shadow_bench -- --ignored --nocapture` (the feature lifts the
//! 200k node cap so the fixture can still grow).
use fagent_storage::*;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const BATCHES: usize = 40;

fn generate(nodes: usize) -> Snapshot {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script = manifest.join("../node-memory/fixtures/generate.mjs");
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("shadow-{nodes}.json"));
    let status = Command::new("node")
        .arg(script)
        .args(["--nodes", &nodes.to_string(), "--seed", "7", "--out"])
        .arg(&out)
        .stdout(std::process::Stdio::null())
        .status()
        .expect("node on PATH");
    assert!(status.success());
    serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap()
}

/// Writes the snapshot's rows straight into a fresh project at revision 0.
fn seed(db: &Path, snapshot: &Snapshot) {
    let mut conn = rusqlite::Connection::open(db).unwrap();
    let tx = conn.transaction().unwrap();
    {
        let mut node = tx
            .prepare(
                "INSERT INTO nodes(id,parent_id,sort_key,kind,props_json) VALUES(?1,?2,?3,?4,?5)",
            )
            .unwrap();
        for n in &snapshot.nodes {
            let kind = serde_json::to_value(&n.kind).unwrap();
            let props = serde_json::to_string(&n.props).unwrap();
            let row = (
                &n.id,
                &n.parent_id,
                n.sort_key,
                kind.as_str().unwrap(),
                props,
            );
            node.execute(row).unwrap();
        }
        let row = |table: &str, id: &str, data: String| {
            let sql = format!("INSERT OR REPLACE INTO {table}(id,data_json) VALUES(?1,?2)");
            tx.execute(&sql, (id, data)).unwrap();
        };
        snapshot
            .tokens
            .iter()
            .for_each(|t| row("tokens", &t.id, text(t)));
        snapshot
            .styles
            .iter()
            .for_each(|s| row("styles", &s.id, text(s)));
        snapshot
            .fonts
            .iter()
            .for_each(|f| row("fonts", &f.id, text(f)));
        snapshot
            .notes
            .iter()
            .for_each(|n| row("notes", &n.id, text(n)));
    }
    assert!(snapshot.assets.is_empty(), "asset files are not seeded");
    tx.commit().unwrap();
}

fn text<T: serde::Serialize>(row: &T) -> String {
    serde_json::to_string(row).unwrap()
}

/// Small edits on rectangles in ordinary frames (not the wide frame, not masters):
/// rename, reorder to the front, create and delete a sibling.
fn batches(snapshot: &Snapshot) -> Vec<Vec<Operation>> {
    let by_id: HashMap<&str, &Node> = snapshot.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let page = |id: &str| {
        let mut node = by_id[id];
        while let Some(parent) = &node.parent_id {
            node = by_id[parent.as_str()];
        }
        node.id.clone()
    };
    let all: Vec<&Node> = snapshot
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Rectangle)
        .filter(|n| n.parent_id.as_deref().is_some_and(|p| p != "frame_wide"))
        .filter(|n| page(&n.id) != "page_library")
        .collect();
    let leaves: Vec<&Node> = all
        .iter()
        .step_by((all.len() / BATCHES).max(1))
        .take(BATCHES)
        .copied()
        .collect();
    assert_eq!(leaves.len(), BATCHES);
    let op = |v: serde_json::Value| serde_json::from_value::<Operation>(v).unwrap();
    (0..BATCHES)
        .map(|i| {
            let leaf = leaves[i];
            let parent = leaf.parent_id.as_deref().unwrap();
            let extra = format!("shadow_bench_{}", i / 4);
            vec![op(match i % 4 {
                0 => json!({"op":"update_node","id":leaf.id,"props":{"name":format!("bench {i}")}}),
                1 => json!({"op":"reorder_node","id":leaf.id,"index":0}),
                2 => json!({"op":"create_node","id":extra,"kind":"rectangle","parentId":parent,
                            "props":{"width":8,"height":8}}),
                _ => json!({"op":"delete_subtree","id":extra}),
            })]
        })
        .collect()
}

fn median(mut values: Vec<u64>) -> u64 {
    values.sort_unstable();
    values[values.len() / 2]
}

/// Commits every batch; returns per-commit `(apply µs, shadow µs, shadow load µs)`.
fn run(mode: ShadowMode, snapshot: &Snapshot, work: &[Vec<Operation>]) -> Vec<[u64; 3]> {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open_with_shadow(temp.path(), mode).unwrap();
    drop(store.create_project("bench", "Bench").unwrap());
    seed(&temp.path().join("bench/db.sql"), snapshot);
    let p = store.open_project("bench").unwrap();
    let mut out = Vec::new();
    for (revision, ops) in work.iter().enumerate() {
        let request = BatchRequest {
            project_id: p.id().into(),
            expected_revision: Some(revision as u64),
            strict: false,
            idempotency_key: format!("k{revision}"),
            operations: ops.clone(),
        };
        let before = node_memory_shadow_stats();
        let start = Instant::now();
        p.apply_batch(request).unwrap();
        let apply = start.elapsed().as_micros() as u64;
        let after = node_memory_shadow_stats();
        let shadow = after.observe_micros - before.observe_micros;
        out.push([apply, shadow, after.load_micros - before.load_micros]);
    }
    let stats = node_memory_shadow_stats();
    assert_eq!((stats.mismatches, stats.errors), (0, 0));
    out
}

#[test]
#[ignore = "release overhead benchmark of the Task28 shadow; needs Node.js"]
fn shadow_overhead_per_commit() {
    assert!(!cfg!(debug_assertions), "run with --release");
    let nodes = std::env::var("SHADOW_BENCH_NODES").map_or(200_000, |v| v.parse().unwrap());
    let snapshot = generate(nodes);
    let work = batches(&snapshot);
    let off = run(ShadowMode::Off, &snapshot, &work);
    let on = run(ShadowMode::On, &snapshot, &work);
    let column = |rows: &[[u64; 3]], i: usize| rows[1..].iter().map(|r| r[i]).collect::<Vec<_>>();
    let summary = json!({
        "authoredNodes": snapshot.nodes.len(),
        "commits": work.len(),
        "firstCommitShadowUs": on[0][1],
        "firstCommitLoadUs": on[0][2],
        "medianShadowUs": median(column(&on, 1)),
        "maxShadowUs": column(&on, 1).into_iter().max(),
        "medianApplyOffUs": median(column(&off, 0)),
        "medianApplyOnUs": median(column(&on, 0)),
    });
    println!("shadow-bench {summary}");
}
