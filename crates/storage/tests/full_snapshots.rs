//! Latest-revision full snapshots (`snapshot`, `snapshot_with_cache_identity`) come
//! from node memory when it holds the revision and incarnation the database reports.
//! The copy equals the database snapshot byte for byte, including resource order,
//! asset readiness and retention; writes outside the engine (imports) evict, so the
//! next full read loads the project from the database again.
mod postgres_support;

use fagent_storage::*;
use serde_json::{json, Value};

const FONT: &[u8] = include_bytes!("../../../fixtures/fonts/Lato-Regular.ttf");
// Complete 1x1 RGBA PNG.
const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 11, 73, 68, 65, 84, 120, 156, 99, 0, 1, 0, 0, 5, 0, 1, 13, 10,
    45, 180, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

fn commit(p: &ProjectHandle, revision: u64, key: &str, ops: Value) -> u64 {
    p.apply_batch(BatchRequest {
        project_id: p.id().into(),
        expected_revision: Some(revision),
        strict: false,
        idempotency_key: key.into(),
        operations: serde_json::from_value(ops).unwrap(),
    })
    .unwrap()
    .committed_revision
}

fn import(p: &ProjectHandle, revision: u64, key: &str, font: Option<&str>) -> u64 {
    let request = ImportAssetRequest {
        project_id: p.id().into(),
        expected_revision: revision,
        idempotency_key: key.into(),
        media_type: if font.is_some() {
            "font/ttf"
        } else {
            "image/png"
        }
        .into(),
        bytes: if font.is_some() { FONT } else { PNG }.into(),
        font: font.map(|id| FontRegistration {
            id: id.into(),
            family: "Lato".into(),
            weight: 400,
            style: "normal".into(),
            license: Some("OFL-1.1".into()),
        }),
    };
    p.import_asset(request).unwrap().result.committed_revision
}

/// Ids chosen so that insertion order, case and punctuation all disagree with the
/// bytewise order the database returns.
fn seed() -> Value {
    let rect = |id: &str, parent: &str| json!({"op":"create_node","id":id,"kind":"rectangle","parentId":parent,"props":{"width":10,"height":10}});
    json!([
        {"op":"create_node","id":"page","kind":"page","parentId":null,"props":{"name":"Home\0page"}},
        {"op":"create_node","id":"frame","kind":"frame","parentId":"page","props":{"width":100,"height":100}},
        rect("z-last", "frame"), rect("A-first", "frame"), rect("_mid", "frame"),
        {"op":"reorder_node","id":"_mid","index":0},
        {"op":"upsert_token","token":{"id":"zeta","name":"Zeta","type":"number","value":-0.0}},
        {"op":"upsert_token","token":{"id":"Alpha","name":"Alpha","type":"color","value":"#112233"}},
        {"op":"upsert_token","token":{"id":"_x","name":"X","type":"number","value":1.5}},
        {"op":"upsert_style","style":{"id":"s-b","name":"B","kind":"fill","value":"#445566"}},
        {"op":"upsert_style","style":{"id":"S-a","name":"A","kind":"fill","value":"#778899"}},
        {"op":"create_note","id":"n-b","nodeId":"frame","text":"Second","author":"a"},
        {"op":"create_note","id":"N-a","nodeId":"z-last","text":"First\0note","author":"b"},
        {"op":"create_note","id":"n_a","nodeId":"page","text":"Third"},
        {"op":"reply_note","id":"n-b","text":"Reply","author":"c"}
    ])
}

/// One full read: from memory (`hit`) or the database, equal in bytes and identity
/// to a database read made right after it.
fn full(storage: &Storage, id: &str, hit: bool) -> Snapshot {
    let before = storage.memory_stats();
    let (snapshot, identity) = storage.snapshot_with_cache_identity(id).unwrap();
    let after = storage.memory_stats();
    assert_eq!(
        after.full_hits - before.full_hits,
        u64::from(hit),
        "{after:?}"
    );
    assert_eq!(
        after.full_fallbacks - before.full_fallbacks,
        u64::from(!hit)
    );
    let (database, database_identity) = storage.database_snapshot(id).unwrap();
    assert_eq!(identity, database_identity);
    assert_eq!(
        serde_json::to_string(&*snapshot).unwrap(),
        serde_json::to_string(&*database).unwrap()
    );
    assert_eq!(storage.snapshot(id).unwrap(), *database);
    std::sync::Arc::unwrap_or_clone(snapshot)
}

fn exercise(storage: &Storage, p: &ProjectHandle) {
    let id = p.id();
    // A created project is cold: its first full read loads it.
    full(storage, id, false);
    full(storage, id, true);
    let mut revision = commit(p, 0, "seed", seed());
    let seeded = full(storage, id, true);
    assert_eq!(seeded.revision, revision);
    let ids = |values: Vec<&str>| values.into_iter().map(String::from).collect::<Vec<_>>();
    assert_eq!(
        seeded
            .tokens
            .iter()
            .map(|t| t.id.clone())
            .collect::<Vec<_>>(),
        ids(vec!["Alpha", "_x", "zeta"])
    );
    assert_eq!(
        seeded
            .notes
            .iter()
            .map(|n| n.id.clone())
            .collect::<Vec<_>>(),
        ids(vec!["N-a", "n-b", "n_a"])
    );

    // An import is a write outside the engine: the next full read loads it.
    revision = import(p, revision, "png", None);
    let loads = storage.memory_stats().read_loads;
    let imported = full(storage, id, false);
    assert_eq!(imported.revision, revision);
    assert_eq!(imported.assets.len(), 1);
    assert_eq!(imported.assets[0].status, "ready");
    assert_eq!(storage.memory_stats().read_loads, loads + 1);
    full(storage, id, true);

    revision = import(p, revision, "font", Some("lato"));
    let font = full(storage, id, false);
    assert_eq!(font.fonts.iter().filter(|f| f.id == "lato").count(), 1);
    revision = commit(
        p,
        revision,
        "edit",
        json!([
            {"op":"update_node","id":"A-first","props":{"width":11}},
            {"op":"upsert_token","token":{"id":"beta","name":"Beta","type":"number","value":0.0}},
            {"op":"delete_note","id":"n_a"}
        ]),
    );
    let edited = full(storage, id, true);
    assert_eq!(edited.revision, revision);
    assert_eq!(edited.retention, seeded.retention);
}

#[test]
fn sqlite_full_snapshots_from_memory_equal_the_database_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(temp.path()).unwrap();
    let project = store.create_project("full-reads", "Full reads").unwrap();
    let storage: Storage = store.into();
    let p = storage.open_project(project.id()).unwrap();
    exercise(&storage, &p);

    // Asset readiness is part of the SQLite identity: a lost file is reported by the
    // next full read, which reloads memory.
    let snapshot = storage.snapshot(p.id()).unwrap();
    let png = snapshot.assets.iter().find(|a| a.media_type == "image/png");
    let assets = temp.path().join("full-reads").join("assets");
    std::fs::remove_file(assets.join(&png.unwrap().relative_key)).unwrap();
    let missing = full(&storage, p.id(), false);
    let png = missing.assets.iter().find(|a| a.media_type == "image/png");
    assert_eq!(png.unwrap().status, "missing");
    full(&storage, p.id(), true);
}

#[test]
#[ignore = "requires fresh PostgreSQL via scripts/task19-check.sh"]
fn postgres_full_snapshots_from_memory_equal_the_database_snapshot() {
    let storage = Storage::new(postgres_support::backend());
    let p = postgres_support::project(&storage, "full-reads");
    exercise(&storage, &p);
}
