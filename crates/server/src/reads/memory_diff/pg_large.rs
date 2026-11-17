//! Frame-context timing on a generated 200k project in PostgreSQL (a disposable,
//! empty database; never the live cluster). Rows are inserted directly at
//! revision 0, as the SQLite run seeds them. Needs Node.js.
//!
//! `MEMORY_DIFF_PG_URL='host=127.0.0.1 port=<port> dbname=<empty db>' cargo test \
//!   --release -p fagent-server --lib memory_diff::pg_large -- --ignored --nocapture`
use super::fixture::generate;
use super::queries::{Caps, Targets};
use super::realdata::storage;
use super::{timing, Pair};
use fagent_storage::{ReadsMode, ShadowMode, Snapshot};
use serde_json::json;

fn seed(url: &str, project: &str, snapshot: &Snapshot) {
    let mut client = postgres::Client::connect(url, postgres::NoTls).unwrap();
    let mut tx = client.transaction().unwrap();
    tx.batch_execute("SET search_path=fagent_runtime_v1")
        .unwrap();
    let sql = "INSERT INTO entities(project_id,kind,id,data) VALUES($1,$2,$3,$4)";
    let insert = tx.prepare(sql).unwrap();
    let mut rows: Vec<(&str, &str, String)> = Vec::new();
    rows.extend(
        snapshot
            .nodes
            .iter()
            .map(|n| ("node", n.id.as_str(), text(n))),
    );
    rows.extend(
        snapshot
            .tokens
            .iter()
            .map(|t| ("token", t.id.as_str(), text(t))),
    );
    rows.extend(
        snapshot
            .styles
            .iter()
            .map(|s| ("style", s.id.as_str(), text(s))),
    );
    rows.extend(
        snapshot
            .fonts
            .iter()
            .map(|f| ("font", f.id.as_str(), text(f))),
    );
    rows.extend(
        snapshot
            .notes
            .iter()
            .map(|n| ("note", n.id.as_str(), text(n))),
    );
    assert!(snapshot.assets.is_empty(), "asset binaries are not seeded");
    for (kind, id, data) in &rows {
        tx.execute(&insert, &[&project, kind, id, data]).unwrap();
    }
    tx.commit().unwrap();
}

fn text<T: serde::Serialize>(row: &T) -> String {
    serde_json::to_string(row).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Task28 I2 timing on a generated 200k project in a disposable PostgreSQL"]
async fn generated_200k_frame_context_timing_postgres() {
    let url = std::env::var("MEMORY_DIFF_PG_URL").expect("MEMORY_DIFF_PG_URL");
    assert!(!url.contains("2703"), "never the live cluster");
    let nodes = std::env::var("MEMORY_DIFF_NODES").map_or(200_000, |v| v.parse().unwrap());
    let temp = tempfile::tempdir().unwrap();
    let snapshot = generate(nodes, temp.path());
    let setup = storage(&url, ReadsMode::Off, ShadowMode::Off);
    let id = setup
        .create_project("gen200k", "Generated")
        .unwrap()
        .id()
        .to_owned();
    tokio::task::block_in_place(|| seed(&url, &id, &snapshot));
    let targets = Targets::of(
        &snapshot,
        &Caps {
            nested: 0,
            instances: 0,
            virtuals: 0,
        },
    );
    let frame = timing::normal_root(&snapshot, &targets.plain);
    let on = storage(&url, ReadsMode::On, ShadowMode::On);
    let mut pair = Pair::new(on, storage(&url, ReadsMode::Off, ShadowMode::Off), &id);
    let mut report = timing::frame_context(&mut pair, &frame).await;
    report["afterWrite"] = timing::after_writes(&mut pair, &frame, true).await;
    let summary = json!({"authoredNodes": snapshot.nodes.len(), "timing": report});
    println!("memory-timing postgres {summary}");
    assert!(pair.differences.is_empty(), "{:?}", pair.differences);
}
