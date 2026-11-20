//! Task 41 benchmark on PostgreSQL with node memory always on, on a generated project
//! (`WRITES_BENCH_NODES` expanded nodes, default 200k) and, when given, the largest
//! project of a disposable backup restore. Every row is 30 measured runs after two
//! warm-ups (median, p95, min, per-phase medians): small props edit, master edit with
//! many instances, reparent, token edit, note create, page create, the scoped part of
//! `get_design_context` on one frame, a latest-revision full `snapshot()` from memory
//! against `database_snapshot`, the first full read of a cold project (load,
//! admission), and a cold commit (load, admission). A cold case runs on a newly
//! opened backend, whose node memory is empty; opening it and its first database
//! round trip are not timed.
//! Prints counts and timings only, one JSON line per row.
//!
//! `scripts/task19-check.sh cargo test --release -p fagent-storage \
//!   --features large-project-bench --test node_memory_writes_bench -- --ignored --nocapture`
//! Optional `WRITES_BENCH_REALDATA_URL` (a restore on another port, never 2703; only
//! the props edit runs there).
#![cfg(feature = "large-project-bench")]
mod writes_bench_support;

use fagent_storage::read_scope::ReadScope;
use fagent_storage::*;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use writes_bench_support::measure::Samples;
use writes_bench_support::{cases, token_setup, Case, Targets};

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

fn storage(url: &str) -> Storage {
    let config = PostgresConfig::new(url);
    Storage::from_shared(Arc::new(PostgresBackend::with_config(config).unwrap()))
}

/// A project handle and the revision its next request is based on.
struct Writer {
    project: Arc<ProjectHandle>,
    id: String,
    revision: u64,
}

impl Writer {
    fn commit(&mut self, pages: &[String], ops: Value) {
        let project = self.project.clone();
        self.commit_on(&project, pages, ops);
    }

    /// `commit` through another backend's handle of the same project.
    fn commit_on(&mut self, project: &ProjectHandle, pages: &[String], ops: Value) {
        let request: ScopedBatchRequest = serde_json::from_value(json!({"projectId":self.id,
            "expectedRevision":self.revision,
            "idempotencyKey":format!("bench-{}", uuid::Uuid::new_v4().simple()),
            "pageIds":pages,"operations":ops}))
        .unwrap();
        self.revision = project
            .apply_scoped_batch(request)
            .unwrap()
            .committed_revision;
    }
}

/// A newly opened backend (empty node memory) and its handle of `id`. The identity
/// probe opens its database session, so a timed run starts with a connection.
fn cold(url: &str, id: &str) -> (Storage, Arc<ProjectHandle>) {
    let storage = storage(url);
    storage.snapshot_cache_identity(id).unwrap();
    let project = storage.open_project(id).unwrap();
    assert_eq!(
        storage.memory_stats().memory.loads,
        0,
        "memory starts empty"
    );
    (storage, project)
}

/// A cold run loaded and admitted the project once, outside reads.
fn assert_one_load(storage: &Storage, writes: u64, full_fallbacks: u64) {
    let stats = storage.memory_stats();
    assert_eq!(
        (stats.memory.loads, stats.memory.admissions),
        (1, 1),
        "{stats:?}"
    );
    assert_eq!(
        (stats.write_hits, stats.write_loads),
        (0, writes),
        "{stats:?}"
    );
    assert_eq!((stats.full_hits, stats.full_fallbacks), (0, full_fallbacks));
}

fn print(label: &str, nodes: usize, case: &str, report: Value) {
    println!("writes-bench {label} nodes={nodes} case={case:?} {report}");
}

fn run_case(label: &str, nodes: usize, writer: &mut Writer, case: &Case) {
    let report = Samples::collect(|run| writer.commit(&case.pages, (case.ops)(run))).report();
    print(label, nodes, &case.name, report);
}

/// The scoped part of `get_design_context` on one frame, as the server reads it: the
/// memory view at the database revision, the frame closure with instance definitions,
/// then property resolution under the default theme.
fn design_context(storage: &Storage, project: &str, frame: &str) -> usize {
    let read = storage.scoped_snapshot(project, frame).unwrap();
    assert!(read.from_memory, "the read must be served from memory");
    let scoped = read.shared.scoped().unwrap();
    let mut nodes = scoped.expand(ReadScope::Target(frame), true).unwrap();
    scoped.resolve(&mut nodes, "default").unwrap().len()
}

fn generated(url: &str, nodes: usize) {
    let snapshot = generate(nodes);
    let authored = snapshot.nodes.len();
    let targets = Targets::new(&snapshot);
    let setup = storage(url);
    let id = setup
        .create_project("gen-writes", "Generated")
        .unwrap()
        .id()
        .to_owned();
    seed(url, &id, &snapshot);
    drop(snapshot);
    let storage = storage(url);
    let project = storage.open_project(&id).unwrap();
    let revision = project.snapshot().unwrap().revision;
    let mut writer = Writer {
        project,
        id: id.clone(),
        revision,
    };
    let label = "generated";
    for case in cases(&targets) {
        if case.name.starts_with("token edit") {
            writer.commit(&case.pages, token_setup());
        }
        run_case(label, authored, &mut writer, &case);
    }
    let resident = storage.memory_stats().resident_bytes;
    println!(
        "writes-bench {label} nodes={authored} memory residentBytes={resident} \
         bytesPerAuthoredNode={:.0}",
        resident as f64 / authored as f64
    );
    let mut closure = 0;
    let report = Samples::collect(|_| closure = design_context(&storage, &id, "root_10")).report();
    print(
        label,
        authored,
        &format!("get_design_context root_10 ({closure} resolved nodes)"),
        report,
    );
    full_reads(&storage, url, label, authored, &id);
    let pages = vec![targets.page(&targets.text)];
    let text = targets.text.clone();
    let report = Samples::collect_prepared(
        |_| cold(url, &id),
        |run, (_, project)| {
            let ops = json!([{"op":"update_node","id":text,"props":{"name":format!("cold{run}")}}]);
            writer.commit_on(project, &pages, ops);
        },
        |(storage, _)| assert_one_load(&storage, 1, 0),
    )
    .report();
    print(label, authored, "cold commit (load, admission)", report);
}

/// Latest-revision full reads: `snapshot()` served from the resident copy, the same
/// revision read from the database only, and `snapshot()` on a newly opened backend
/// (the read loads the project into memory and pays the admission).
fn full_reads(storage: &Storage, url: &str, label: &str, authored: usize, id: &str) {
    // The write cases above added nodes; every read must return the current revision.
    let current = storage.database_snapshot(id).unwrap().0.nodes.len();
    let hits = storage.memory_stats().full_hits;
    let report =
        Samples::collect(|_| assert_eq!(storage.snapshot(id).unwrap().nodes.len(), current))
            .report();
    assert_eq!(
        storage.memory_stats().full_hits - hits,
        32,
        "every full read is a memory hit"
    );
    print(label, authored, "full snapshot() memory hit", report);
    let report = Samples::collect(|_| {
        assert_eq!(
            storage.database_snapshot(id).unwrap().0.nodes.len(),
            current
        )
    })
    .report();
    print(label, authored, "full database_snapshot", report);
    let report = Samples::collect_prepared(
        |_| cold(url, id).0,
        |_, storage| assert_eq!(storage.snapshot(id).unwrap().nodes.len(), current),
        |storage| {
            assert_one_load(&storage, 0, 1);
            assert_eq!(storage.memory_stats().read_loads, 1, "the read loaded it");
        },
    )
    .report();
    print(
        label,
        authored,
        "cold full snapshot() (load, admission)",
        report,
    );
}

/// Props edits on the largest project of a restore, through the same scoped request.
fn real(url: &str) {
    let storage = storage(url);
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
    let node = largest
        .nodes
        .iter()
        .find(|n| n.parent_id.is_some() && n.kind != NodeKind::Instance)
        .expect("a non-page node");
    let mut page = node.id.as_str();
    let parent: std::collections::HashMap<&str, Option<&str>> = largest
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n.parent_id.as_deref()))
        .collect();
    while let Some(Some(p)) = parent.get(page) {
        page = p;
    }
    let mut writer = Writer {
        project: storage.open_project(&largest.project_id).unwrap(),
        id: largest.project_id.clone(),
        revision: largest.revision,
    };
    let (node, pages) = (node.id.clone(), vec![page.to_owned()]);
    let report = Samples::collect(|run| {
        writer.commit(
            &pages,
            json!([{"op":"update_node","id":node,"props":{"name":format!("w{run}")}}]),
        )
    })
    .report();
    print("real", largest.nodes.len(), "props edit", report);
}

#[test]
#[ignore = "commit timing on a disposable PostgreSQL (scripts/task19-check.sh)"]
fn commit_latency() {
    let url = std::env::var("TASK19_POSTGRES_URL").expect("TASK19_POSTGRES_URL");
    assert!(!url.contains(":2703"), "never the live cluster");
    let nodes = std::env::var("WRITES_BENCH_NODES").map_or(200_000, |v| v.parse().unwrap());
    generated(&url, nodes);
    if let Ok(real_url) = std::env::var("WRITES_BENCH_REALDATA_URL") {
        assert!(!real_url.contains("2703"), "never the live cluster");
        real(&real_url);
    }
}
