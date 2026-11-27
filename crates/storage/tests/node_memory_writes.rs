//! Node memory is always on. Two databases with identical data, one served by a
//! storage that keeps its memory warm and one by a newly opened (cold) storage for
//! every request, give the same results and the same stored rows on SQLite and
//! PostgreSQL; after every commit both storages serve the database's state from
//! memory, and batches at the current base match `Candidate` apply + `validate()`.
//! Random batches come from agents with stale bases, strict and scoped requests,
//! rejections, replays and previews; writers also race on threads (replayed
//! afterwards in commit order).
mod memory_writes_support;
mod node_memory_i4;
mod node_memory_i5;
mod postgres_support;

use fagent_storage::*;
use memory_writes_support::{gen, rows, Pair};
use serde_json::{json, Value};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        match entry.file_type().unwrap().is_dir() {
            true => copy_dir(&entry.path(), &target),
            false => drop(std::fs::copy(entry.path(), target).unwrap()),
        }
    }
}

/// The same project file under two data roots, `warm` and `cold`.
fn sqlite_pair(temp: &Path) -> Pair {
    let (warm_root, cold_root) = (temp.join("warm"), temp.join("cold"));
    let project = {
        let store: Storage = Store::open(&cold_root).unwrap().into();
        let project = store.create_project("diff", "Diff").unwrap();
        project.id().to_owned()
    };
    copy_dir(&cold_root.join("diff"), &warm_root.join("diff"));
    let db = |root: &PathBuf| root.join("diff/db.sql");
    let (warm_db, cold_db) = (db(&warm_root), db(&cold_root));
    Pair {
        warm: Store::open(&warm_root).unwrap().into(),
        cold: Box::new(move || Store::open(&cold_root).unwrap().into()),
        project,
        rows: Box::new(move |warm| rows::sqlite(if warm { &warm_db } else { &cold_db })),
        cold_writes: Cell::default(),
    }
}

fn postgres_store(url: &str) -> Storage {
    let config = PostgresConfig::new(url);
    Storage::from_shared(Arc::new(PostgresBackend::with_config(config).unwrap()))
}

/// The same project row in the task database (`cold`) and in a fresh second
/// database (`warm`).
fn postgres_pair(label: &str) -> Pair {
    let url = postgres_support::url();
    let name = format!("i3_{}", uuid::Uuid::new_v4().simple());
    postgres_support::sql()
        .batch_execute(&format!("CREATE DATABASE {name}"))
        .unwrap();
    let (base, _) = url.rsplit_once('/').unwrap();
    let other = format!("{base}/{name}");
    let cold = postgres_store(&url);
    let warm = postgres_store(&other);
    let project = postgres_support::project(&cold, label).id().to_owned();
    let copy = "SELECT project_id,slug,name,schema_version,revision,oldest_revision FROM fagent_runtime_v1.projects WHERE project_id=$1";
    let row = postgres_support::sql()
        .query_one(copy, &[&project])
        .unwrap();
    let mut target = postgres::Client::connect(&other, postgres::NoTls).unwrap();
    target.execute("INSERT INTO fagent_runtime_v1.projects(project_id,slug,name,schema_version,revision,oldest_revision) VALUES($1,$2,$3,$4,$5,$6)",
        &[&row.get::<_, String>(0), &row.get::<_, String>(1), &row.get::<_, Vec<u8>>(2), &row.get::<_, i32>(3), &row.get::<_, i64>(4), &row.get::<_, i64>(5)]).unwrap();
    // Rows created with the project (the bundled font) at revision 0.
    let entities = "SELECT kind,id,data FROM fagent_runtime_v1.entities WHERE project_id=$1";
    for row in postgres_support::sql()
        .query(entities, &[&project])
        .unwrap()
    {
        let (kind, id, data): (String, String, String) = (row.get(0), row.get(1), row.get(2));
        target.execute("INSERT INTO fagent_runtime_v1.entities(project_id,kind,id,data) VALUES($1,$2,$3,$4)", &[&project, &kind, &id, &data]).unwrap();
    }
    drop(cold);
    let (id, cold_url) = (project.clone(), url.clone());
    Pair {
        warm,
        cold: Box::new(move || postgres_store(&cold_url)),
        project,
        rows: Box::new(move |warm| rows::postgres(if warm { &other } else { &url }, &id)),
        cold_writes: Cell::default(),
    }
}

/// Returns the totals; callers whose run is long enough also assert coverage.
fn differential(
    pair: &Pair,
    seeds: std::ops::Range<u64>,
    steps: usize,
) -> memory_writes_support::Tally {
    pair.seed();
    let mut total = memory_writes_support::Tally::default();
    for seed in seeds {
        let tally = pair.run(seed, steps);
        total.committed += tally.committed;
        total.rejected += tally.rejected;
        total.conflicts += tally.conflicts;
        total.concurrent += tally.concurrent;
        total.replays += tally.replays;
        total.previews += tally.previews;
        total.oracle += tally.oracle;
        total.deleted_notes += tally.deleted_notes;
    }
    let (warm, (cold_hits, cold_loads)) = (pair.warm.memory_stats(), pair.cold_writes.get());
    eprintln!(
        "node-memory writes differential: {total:?}\n  warm {warm:?}\n  \
         cold writeHits={cold_hits} writeLoads={cold_loads}"
    );
    assert!(total.committed > 0 && total.rejected > 0 && total.conflicts > 0);
    assert!(
        total.concurrent > 0,
        "stale bases must report concurrent writes"
    );
    assert!(total.oracle > 0, "batches at the current base were checked");
    // The warm storage staged in its resident copy; the cold ones loaded every time.
    assert!(warm.write_hits > warm.write_loads, "{warm:?}");
    assert_eq!(cold_hits, 0, "cold storages start empty");
    assert!(cold_loads > 0, "cold storages loaded the project");
    total
}

/// Writers race on threads against `warm`; replaying their requests in the order
/// they committed against `cold` must give the same results and rows.
fn racing(pair: &Pair) {
    pair.seed();
    let requests: Vec<Value> = (0..4)
        .flat_map(|writer| (0..12).map(move |i| (writer, i)))
        .map(|(writer, i)| {
            let mut rng = gen::Rng::new(writer * 100 + i);
            let mut ids = gen::Ids::default();
            let ops: Vec<Value> = (0..2).map(|_| gen::operation(&mut rng, &mut ids)).collect();
            json!({"projectId":pair.project,"expectedRevision":1,"idempotencyKey":format!("w{writer}-{i}"),
                "operations":ops,"strict":i % 5 == 4})
        })
        .collect();
    let handles: Vec<_> = requests
        .chunks(12)
        .map(|chunk| {
            let (warm, project, chunk) = (pair.warm.clone(), pair.project.clone(), chunk.to_vec());
            std::thread::spawn(move || {
                let p = warm.open_project(&project).unwrap();
                chunk
                    .into_iter()
                    .map(|request| {
                        let r: BatchRequest = serde_json::from_value(request.clone()).unwrap();
                        let result = p
                            .apply_batch(r)
                            .map_err(|e| serde_json::to_value(e).unwrap());
                        (request, result.map(|r| serde_json::to_value(r).unwrap()))
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let mut done: Vec<(Value, std::result::Result<Value, Value>)> = handles
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();
    let committed = |r: &std::result::Result<Value, Value>| {
        r.as_ref()
            .ok()
            .map(|v| v["committedRevision"].as_u64().unwrap())
    };
    done.sort_by_key(|(_, r)| committed(r).unwrap_or(u64::MAX));
    let mut last = None;
    for (request, warm) in done.iter().filter(|(_, r)| r.is_ok()) {
        let r: BatchRequest = serde_json::from_value(request.clone()).unwrap();
        drop(last.take());
        let fresh = (pair.cold)();
        let p = fresh.open_project(&pair.project).unwrap();
        let cold = p.apply_batch(r).map(|r| serde_json::to_value(r).unwrap());
        pair.count_cold(&fresh);
        last = Some(fresh);
        assert_eq!(
            cold.ok().as_ref(),
            warm.as_ref().ok(),
            "replayed {}",
            request["idempotencyKey"]
        );
    }
    pair.assert_rows(0);
    let cold = last.expect("replayed commits");
    for storage in [&pair.warm, &cold] {
        memory_writes_support::assert_memory_matches_database(storage, &pair.project, "racing");
    }
    assert_eq!(
        pair.cold_writes.get().0,
        0,
        "every replay loaded the project"
    );
}

#[test]
fn sqlite_warm_and_cold_memory_writes_agree() {
    let temp = tempfile::tempdir().unwrap();
    let total = differential(&sqlite_pair(temp.path()), 1..4, 300);
    // Checked on every oracle batch; this longer run is sure to reach a cascade.
    assert!(
        total.deleted_notes > 0,
        "node deletions reported their notes"
    );
}

#[test]
fn sqlite_racing_writers_match_a_sequential_replay() {
    let temp = tempfile::tempdir().unwrap();
    racing(&sqlite_pair(temp.path()));
}

#[test]
#[ignore = "requires fresh PostgreSQL via scripts/task19-check.sh"]
fn postgres_warm_and_cold_memory_writes_agree() {
    differential(&postgres_pair("i3-diff"), 1..3, 200);
}

#[test]
#[ignore = "requires fresh PostgreSQL via scripts/task19-check.sh"]
fn postgres_racing_writers_match_a_sequential_replay() {
    racing(&postgres_pair("i3-race"));
}
