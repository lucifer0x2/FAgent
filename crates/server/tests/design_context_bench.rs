//! Task 41: the whole `get_design_context` tool call on one frame, through the MCP
//! listener (read, scoped expansion and resolution, viewports, diagnostics, JSON
//! shaping and the response), on a generated project in a temporary SQLite root.
//! 30 measured calls after two warm-ups (median, p95), right after load and right
//! after a write. Prints counts and timings only, one line per row.
//!
//! `cargo test --release -p fagent-server --test design_context_bench -- --ignored --nocapture`
//! `DESIGN_CONTEXT_BENCH_NODES` sets the expanded size (default 7,000; a project over
//! `MAX_NODES` needs `--features large-project-bench`).
#[allow(dead_code)]
mod support;

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use support::Server;

const WARM_UPS: usize = 2;
const RUNS: usize = 30;

fn generate(nodes: usize) -> Value {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("context-{nodes}.json"));
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

/// Writes the generated rows into the project's database, as stored rows.
fn seed(db: &Path, snapshot: &Value) {
    let mut conn = rusqlite::Connection::open(db).unwrap();
    let tx = conn.transaction().unwrap();
    {
        let mut node = tx
            .prepare(
                "INSERT INTO nodes(id,parent_id,sort_key,kind,props_json) VALUES(?1,?2,?3,?4,?5)",
            )
            .unwrap();
        for n in snapshot["nodes"].as_array().unwrap() {
            let props = serde_json::to_string(&n["props"]).unwrap();
            let row = (&n["id"], &n["parentId"], &n["sortKey"], &n["kind"]);
            node.execute(rusqlite::params![
                row.0.as_str(),
                row.1.as_str(),
                row.2.as_i64(),
                row.3.as_str(),
                props
            ])
            .unwrap();
        }
        for table in ["tokens", "styles", "fonts", "notes"] {
            let sql = format!("INSERT OR REPLACE INTO {table}(id,data_json) VALUES(?1,?2)");
            let mut insert = tx.prepare(&sql).unwrap();
            for row in snapshot[table].as_array().into_iter().flatten() {
                let data = serde_json::to_string(row).unwrap();
                insert
                    .execute(rusqlite::params![row["id"].as_str(), data])
                    .unwrap();
            }
        }
    }
    tx.commit().unwrap();
}

/// Median and p95 of `RUNS` calls after `WARM_UPS`, in ms, and the last response size.
async fn measure(server: &Server, args: &Value) -> (f64, f64, usize) {
    let mut values = Vec::new();
    let mut bytes = 0;
    for run in 0..WARM_UPS + RUNS {
        let start = Instant::now();
        let result = server.call("get_design_context", args.clone()).await;
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        assert!(result.get("nodes").is_some(), "{result}");
        bytes = serde_json::to_vec(&result).unwrap().len();
        if run >= WARM_UPS {
            values.push(elapsed);
        }
    }
    values.sort_by(f64::total_cmp);
    let p95 = values[(values.len() * 95).div_ceil(100) - 1];
    (values[values.len() / 2], p95, bytes)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "release benchmark: --ignored --nocapture"]
#[allow(clippy::assertions_on_constants)] // Runtime guard keeps debug test compilation valid.
async fn design_context_one_frame() {
    assert!(!cfg!(debug_assertions), "benchmark requires --release");
    let nodes = std::env::var("DESIGN_CONTEXT_BENCH_NODES")
        .map_or(7_000, |v| v.parse().expect("a node count"));
    let snapshot = generate(nodes);
    let authored = snapshot["nodes"].as_array().unwrap().len();
    let server = Server::start().await;
    let project = server.project().await;
    seed(&server.root.path().join("test-project/db.sql"), &snapshot);
    drop(snapshot);
    let args = json!({"projectId": project, "frameId": "root_10"});
    let print = |case: &str, (median, p95, bytes): (f64, f64, usize)| {
        println!(
            "design-context-bench nodes={authored} case={case:?} medianMs={median:.2} \
             p95Ms={p95:.2} responseBytes={bytes}"
        );
    };
    print("get_design_context root_10", measure(&server, &args).await);
    let mut revision = server
        .call("get_outline", json!({"projectId": project}))
        .await["revision"]
        .as_u64()
        .unwrap();
    let mut values = Vec::new();
    for run in 0..WARM_UPS + RUNS {
        let name = format!("Renamed {run}");
        let ops = json!([{"op":"update_node","id":"root_10","props":{"name":name},"merge":true}]);
        let committed = server
            .batch(&project, revision, &format!("rename-{run}"), ops)
            .await;
        revision = committed["committedRevision"]
            .as_u64()
            .unwrap_or_else(|| panic!("{committed}"));
        let start = Instant::now();
        server.call("get_design_context", args.clone()).await;
        if run >= WARM_UPS {
            values.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    values.sort_by(f64::total_cmp);
    let p95 = values[(values.len() * 95).div_ceil(100) - 1];
    print(
        "get_design_context root_10 right after a write",
        (values[values.len() / 2], p95, 0),
    );
}
