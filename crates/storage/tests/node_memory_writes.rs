//! Task28 I3: commits through memory (`WritesMode`) give the same results and the
//! same stored rows as today's path, on SQLite and PostgreSQL: random batches from
//! agents with stale bases, strict and scoped requests, rejections, replays,
//! previews, and writers racing on threads (replayed afterwards in commit order).
mod memory_writes_support;
mod node_memory_i4;
mod node_memory_i5;
mod postgres_support;

use fagent_storage::*;
use memory_writes_support::{gen, rows, Pair};
use serde_json::{json, Value};
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

fn sqlite_store(root: &Path, on: bool) -> Storage {
    let store = match on {
        true => Store::open_with_node_memory_writes(
            root,
            ShadowMode::Strict,
            ReadsMode::On,
            WritesMode::Strict,
        ),
        false => Store::open_with_node_memory_writes(
            root,
            ShadowMode::Off,
            ReadsMode::Off,
            WritesMode::Off,
        ),
    };
    store.unwrap().into()
}

/// The same project file under two data roots.
fn sqlite_pair(temp: &Path) -> Pair {
    let (off_root, on_root) = (temp.join("off"), temp.join("on"));
    let project = {
        let store = sqlite_store(&off_root, false);
        let project = store
            .create_project("diff", "Diff")
            .unwrap()
            .id()
            .to_owned();
        project
    };
    copy_dir(&off_root.join("diff"), &on_root.join("diff"));
    let db = |root: &PathBuf| root.join("diff/db.sql");
    let (off_db, on_db) = (db(&off_root), db(&on_root));
    Pair {
        off: sqlite_store(&off_root, false),
        on: sqlite_store(&on_root, true),
        project,
        rows: Box::new(move |on| rows::sqlite(if on { &on_db } else { &off_db })),
    }
}

fn postgres_store(url: &str, on: bool) -> Storage {
    let mut config = PostgresConfig::new(url);
    let (shadow, reads, writes) = match on {
        true => (ShadowMode::Strict, ReadsMode::On, WritesMode::Strict),
        false => (ShadowMode::Off, ReadsMode::Off, WritesMode::Off),
    };
    config.node_memory_shadow = shadow;
    config.node_memory_reads = reads;
    config.node_memory_writes = writes;
    Storage::from_shared(Arc::new(PostgresBackend::with_config(config).unwrap()))
}

/// The same project row in the task database and in a fresh second database.
fn postgres_pair(label: &str) -> Pair {
    let url = postgres_support::url();
    let name = format!("i3_{}", uuid::Uuid::new_v4().simple());
    postgres_support::sql()
        .batch_execute(&format!("CREATE DATABASE {name}"))
        .unwrap();
    let (base, _) = url.rsplit_once('/').unwrap();
    let other = format!("{base}/{name}");
    let off = postgres_store(&url, false);
    let on = postgres_store(&other, true);
    let project = postgres_support::project(&off, label).id().to_owned();
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
    let id = project.clone();
    Pair {
        off,
        on,
        project,
        rows: Box::new(move |on| rows::postgres(if on { &other } else { &url }, &id)),
    }
}

fn differential(pair: &Pair, seeds: std::ops::Range<u64>, steps: usize) {
    let start = node_memory_shadow_stats();
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
    }
    let end = node_memory_shadow_stats();
    eprintln!(
        "node-memory writes differential: {total:?}, memory bases {} misses {} fallbacks {} aborts {} loads {} evictions {}",
        end.write_hits - start.write_hits,
        end.write_misses - start.write_misses,
        end.write_fallbacks - start.write_fallbacks,
        end.write_aborts - start.write_aborts,
        end.loads - start.loads,
        end.evictions - start.evictions,
    );
    assert!(total.committed > 0 && total.rejected > 0 && total.conflicts > 0);
    assert!(
        total.concurrent > 0,
        "stale bases must report concurrent writes"
    );
    assert!(end.write_hits - start.write_hits >= total.committed as u64 / 2);
}

/// Writers race on threads against `on`; replaying their requests in the order
/// they committed against `off` must give the same results and rows.
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
            let (on, project, chunk) = (pair.on.clone(), pair.project.clone(), chunk.to_vec());
            std::thread::spawn(move || {
                let p = on.open_project(&project).unwrap();
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
    let p = pair.off.open_project(&pair.project).unwrap();
    for (request, on) in done.iter().filter(|(_, r)| r.is_ok()) {
        let r: BatchRequest = serde_json::from_value(request.clone()).unwrap();
        let off = p.apply_batch(r).map(|r| serde_json::to_value(r).unwrap());
        assert_eq!(
            off.ok().as_ref(),
            on.as_ref().ok(),
            "replayed {}",
            request["idempotencyKey"]
        );
    }
    pair.assert_rows(0);
}

#[test]
fn sqlite_memory_writes_match_todays_path() {
    let temp = tempfile::tempdir().unwrap();
    differential(&sqlite_pair(temp.path()), 1..4, 300);
}

#[test]
fn sqlite_racing_writers_match_a_sequential_replay() {
    let temp = tempfile::tempdir().unwrap();
    racing(&sqlite_pair(temp.path()));
}

#[test]
#[ignore = "requires fresh PostgreSQL via scripts/task19-check.sh"]
fn postgres_memory_writes_match_todays_path() {
    differential(&postgres_pair("i3-diff"), 1..3, 200);
}

#[test]
#[ignore = "requires fresh PostgreSQL via scripts/task19-check.sh"]
fn postgres_racing_writers_match_a_sequential_replay() {
    racing(&postgres_pair("i3-race"));
}
