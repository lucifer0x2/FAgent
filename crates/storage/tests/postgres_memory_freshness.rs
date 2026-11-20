//! Two backends on one PostgreSQL namespace stand for two processes, each with its
//! own node memory: neither reads, validates nor previews against a revision the
//! other one has moved past.
mod postgres_support;
use fagent_storage::*;
use postgres_support::*;
use serde_json::{json, Value};

fn ops(value: Value) -> Vec<Operation> {
    serde_json::from_value(value).unwrap()
}

fn strict(mut request: BatchRequest) -> BatchRequest {
    request.strict = true;
    request
}

fn commit(storage: &Storage, request: BatchRequest) -> u64 {
    let p = storage.open_project(&request.project_id).unwrap();
    p.apply_batch(request).unwrap().committed_revision
}

/// `(from_memory, revision, page name)` of a scoped read of the page.
fn read(storage: &Storage, id: &str) -> (bool, u64, Value) {
    let read = storage.scoped_snapshot(id, "page").unwrap();
    let scoped = read.shared.scoped().unwrap();
    let name = scoped.node("page").unwrap().props["name"].clone();
    (read.from_memory, read.shared.snapshot.revision, name)
}

/// The latest full snapshot is served from memory and, node order aside, equals the
/// database's.
fn assert_memory_matches_database(storage: &Storage, id: &str) {
    let sorted = |mut snapshot: Snapshot| {
        snapshot.nodes.sort_by(|a, b| a.id.cmp(&b.id));
        normalized(snapshot)
    };
    let hits = storage.memory_stats().full_hits;
    let memory = storage.snapshot(id).unwrap();
    assert_eq!(storage.memory_stats().full_hits, hits + 1, "resident");
    let database = storage.database_snapshot(id).unwrap().0.as_ref().clone();
    assert_eq!(sorted(memory), sorted(database));
}

#[test]
#[ignore = "requires fresh PostgreSQL via scripts/task19-check.sh"]
fn another_processes_commits_reach_reads_batches_and_previews() {
    let (_, a) = setup();
    let (_, b) = setup();
    let id = project(&a, "two-processes").id().to_owned();
    let mut seed = page("Seed");
    seed.extend(ops(json!([
        {"op":"create_node","id":"box","kind":"frame","parentId":"page","props":{"width":10,"height":10}}
    ])));
    assert_eq!(commit(&a, request(&id, 0, "seed", seed)), 1);
    assert_eq!(
        read(&a, &id),
        (true, 1, json!("Seed")),
        "resident after commit"
    );

    // B deletes the frame A still holds in memory.
    let delete = ops(json!([{"op":"delete_subtree","id":"box"}]));
    assert_eq!(commit(&b, request(&id, 1, "delete", delete)), 2);
    let loads = a.memory_stats().write_loads;

    // A's strict batch at the old base conflicts at B's revision.
    let stale = a.open_project(&id).unwrap();
    let error = stale
        .apply_batch(strict(request(&id, 1, "stale", rename("Stale"))))
        .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.current_revision),
        ("REVISION_CONFLICT", Some(2))
    );
    // A validates against B's revision, where the frame is gone.
    let gone = ops(json!([{"op":"update_node","id":"box","props":{"name":"Gone"}}]));
    let error = stale
        .apply_batch(request(&id, 2, "gone", gone.clone()))
        .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.op_index),
        ("VALIDATION_FAILED", Some(0)),
        "{error:?}"
    );
    let preview = stale.dry_run_batch(request(&id, 2, "gone-preview", gone));
    assert_eq!(preview.unwrap_err().code, "VALIDATION_FAILED");
    // At B's revision A commits and its memory holds the database's state.
    assert_eq!(commit(&a, request(&id, 2, "new", rename("New"))), 3);
    assert!(
        a.memory_stats().write_loads > loads,
        "A reloaded B's revision"
    );
    assert_memory_matches_database(&a, &id);
    assert_eq!(read(&a, &id), (true, 3, json!("New")));

    // B (resident at 2) and A's reads follow each other's next commits.
    assert_eq!(commit(&b, request(&id, 3, "b", rename("B"))), 4);
    assert_memory_matches_database(&b, &id);
    assert_eq!(read(&a, &id), (false, 4, json!("B")), "behind: reload");
    assert_eq!(read(&a, &id), (true, 4, json!("B")));
    assert_memory_matches_database(&a, &id);

    // A's previews see B's revision too.
    let preview = a.open_project(&id).unwrap();
    let error = preview
        .dry_run_batch(strict(request(&id, 3, "old", rename("Old"))))
        .unwrap_err();
    assert_eq!(error.current_revision, Some(4));
    let current = preview
        .dry_run_batch(strict(request(&id, 4, "current", rename("Current"))))
        .unwrap();
    assert_eq!(current.base_revision, 4);
}
