mod postgres_support;
use fagent_storage::*;
use postgres_support::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;

const FONT: &[u8] = include_bytes!("../../../fixtures/fonts/Lato-Regular.ttf");
// Complete 1x1 RGBA PNG.
const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 11, 73, 68, 65, 84, 120, 156, 99, 0, 1, 0, 0, 5, 0, 1, 13, 10,
    45, 180, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

fn ops(value: Value) -> Vec<Operation> {
    serde_json::from_value(value).unwrap()
}

fn import(
    id: &str,
    revision: u64,
    key: &str,
    bytes: &[u8],
    font: Option<&str>,
) -> ImportAssetRequest {
    ImportAssetRequest {
        project_id: id.into(),
        expected_revision: revision,
        idempotency_key: key.into(),
        media_type: if font.is_some() {
            "font/ttf"
        } else {
            "image/png"
        }
        .into(),
        bytes: bytes.into(),
        font: font.map(|id| FontRegistration {
            id: id.into(),
            family: "Lato".into(),
            weight: 400,
            style: "normal".into(),
            license: Some("OFL-1.1".into()),
        }),
    }
}

/// A full read with the entity tables locked: served from node memory, or a busy
/// error when the read needed SQL.
fn resident(storage: &Storage, id: &str) -> std::result::Result<Arc<Snapshot>, String> {
    let mut control = sql();
    let lock = lock_entity_tables(&mut control);
    let read = storage.snapshot_with_cache_identity(id);
    lock.rollback().unwrap();
    read.map(|(snapshot, _)| snapshot)
        .map_err(|error| error.code)
}

/// The writer's memory serves `revision` without SQL, identical (also in bytes) to a
/// database read of another backend.
fn assert_resident(storage: &Storage, fresh: &Storage, id: &str, revision: u64) -> Snapshot {
    let hot = resident(storage, id).unwrap();
    let (cold, identity) = fresh.database_snapshot(id).unwrap();
    assert_eq!(hot.revision, revision);
    assert_eq!(identity.unwrap().revision, revision);
    assert_eq!(*hot, *cold);
    // Domain equality treats -0.0 and 0.0 as equal; serialized bytes do not.
    assert_eq!(
        serde_json::to_string(&*hot).unwrap(),
        serde_json::to_string(&*cold).unwrap()
    );
    Arc::unwrap_or_clone(cold)
}

/// A write outside the engine evicted the project: the next full read loads it from
/// SQL and adopts it, and the read after that is served from memory.
fn assert_evicted_then_loaded(
    storage: &Storage,
    fresh: &Storage,
    id: &str,
    revision: u64,
) -> Snapshot {
    assert_eq!(resident(storage, id).unwrap_err(), "STORAGE_BUSY");
    let loaded = storage.snapshot(id).unwrap();
    let hot = assert_resident(storage, fresh, id, revision);
    assert_eq!(hot, loaded);
    hot
}

fn zero(value: Value) -> Vec<Operation> {
    ops(json!([
        {"op":"upsert_token","token":{"id":"zero","name":"Zero","type":"number","value":value}},
        {"op":"update_node","id":"box","props":{"shadow":[{"x":value,"color":"#000000"}]}}
    ]))
}

#[test]
#[ignore = "requires fresh PostgreSQL via scripts/task19-check.sh"]
fn every_write_is_visible_to_the_next_full_read() {
    let storage = Storage::new(backend());
    let (_, fresh) = setup();
    let p = project(&storage, "full-read-writes");
    let id = p.id();
    let mut seed = page("Seed");
    seed.extend(ops(json!([
        {"op":"create_node","id":"frame","kind":"frame","parentId":"page","props":{"width":100,"height":100}},
        {"op":"create_node","id":"box","kind":"rectangle","parentId":"frame","props":{"width":10,"height":10}},
        {"op":"create_node","id":"gone","kind":"rectangle","parentId":"frame","props":{"width":5,"height":5}},
        {"op":"upsert_style","style":{"id":"fill","name":"Fill","kind":"fill","value":"#112233"}},
        {"op":"create_note","id":"note","nodeId":"box","text":"Note\0text","author":"a"}
    ])));
    seed.extend(zero(json!(-0.0)));
    let mut revision = p
        .apply_batch(request(id, 0, "seed", seed))
        .unwrap()
        .committed_revision;
    assert_resident(&storage, &fresh, id, revision);

    let batches = [
        ops(json!([
            {"op":"update_node","id":"box","props":{"width":20}},
            {"op":"delete_subtree","id":"gone"},
            {"op":"create_node","id":"first","kind":"rectangle","parentId":"frame","index":0,"props":{"width":1,"height":1}}
        ])),
        ops(json!([
            {"op":"upsert_token","token":{"id":"accent","name":"Accent","type":"color","value":"#abcdef"}},
            {"op":"upsert_style","style":{"id":"fill","name":"Fill","kind":"fill","value":"#445566"}},
            {"op":"reply_note","id":"note","text":"Reply","author":"b"}
        ])),
        ops(json!([{"op":"delete_note","id":"note"}])),
        zero(json!(0.0)),
        zero(json!(-0.0)),
    ];
    for (index, operations) in batches.into_iter().enumerate() {
        let key = format!("batch-{index}");
        revision = p
            .apply_batch(request(id, revision, &key, operations))
            .unwrap()
            .committed_revision;
        assert_resident(&storage, &fresh, id, revision);
    }
    let signed = resident(&storage, id).unwrap();
    let token = signed
        .tokens
        .iter()
        .find(|token| token.id == "zero")
        .unwrap();
    assert_eq!(serde_json::to_string(&token.value).unwrap(), "-0.0");

    let imports = [
        ("png-new", PNG, None),
        ("png-present", PNG, None),
        ("font-new", FONT, Some("lato")),
        ("font-present", FONT, Some("lato-copy")),
    ];
    for (key, bytes, font) in imports {
        let imported = p
            .import_asset(import(id, revision, key, bytes, font))
            .unwrap();
        revision = imported.result.committed_revision;
        let hot = assert_evicted_then_loaded(&storage, &fresh, id, revision);
        assert!(hot.assets.iter().all(|asset| asset.status == "ready"));
    }

    // PostgreSQL identities carry no asset readiness: a binary removed outside the
    // domain is reported missing by the next load of the project.
    let png_hash = format!("{:x}", Sha256::digest(PNG));
    sql()
        .execute(
            "DELETE FROM fagent_runtime_v1.binaries WHERE project_id=$1 AND content_hash=$2",
            &[&id, &png_hash],
        )
        .unwrap();
    let operations = ops(json!([{"op":"update_node","id":"page","props":{"name":"Renamed"}}]));
    revision = p
        .apply_batch(request(id, revision, "after-missing", operations))
        .unwrap()
        .committed_revision;
    let reloaded = Storage::new(backend());
    let hot = assert_evicted_then_loaded(&reloaded, &fresh, id, revision);
    let png = hot
        .assets
        .iter()
        .find(|a| a.content_hash == png_hash)
        .unwrap();
    assert_eq!(png.status, "missing");
    let fonts: Vec<_> = hot
        .fonts
        .iter()
        .map(|f| (f.id.as_str(), f.status.as_str()))
        .collect();
    assert!(fonts.contains(&("lato", "ready")));
    assert!(fonts.contains(&("lato-copy", "ready")));
}

#[test]
#[ignore = "requires fresh PostgreSQL via scripts/task19-check.sh"]
fn replays_dry_runs_failures_and_conflicts_keep_the_resident_revision() {
    let writer = Storage::new(backend());
    let p = project(&writer, "no-commit");
    let id = p.id();
    let seed = request(id, 0, "seed", page("Seed"));
    p.apply_batch(seed.clone()).unwrap();
    let asset = import(id, 1, "png", PNG, None);
    p.import_asset(asset.clone()).unwrap();
    assert_eq!(resident(&writer, id).unwrap_err(), "STORAGE_BUSY");
    writer.snapshot(id).unwrap();
    let warm = resident(&writer, id).unwrap();
    assert_eq!(warm.revision, 2);

    let invalid = ops(json!([{"op":"update_node","id":"absent","props":{"name":"x"}}]));
    let invalid = request(id, 2, "invalid", invalid);
    // Only strict requests conflict on a stale base; others commit last-writer-wins.
    let mut stale = request(id, 1, "stale", page("Stale"));
    stale.strict = true;
    let dry = request(id, 2, "dry", rename("Dry"));
    let outcomes = |handle: &ProjectHandle| {
        assert!(handle.apply_batch(seed.clone()).unwrap().replayed);
        assert!(handle.import_asset(asset.clone()).unwrap().result.replayed);
        assert!(handle.dry_run_batch(dry.clone()).unwrap().dry_run);
        let failed = handle.apply_batch(invalid.clone()).unwrap_err();
        assert_eq!(failed.code, "VALIDATION_FAILED");
        let conflict = handle.apply_batch(stale.clone()).unwrap_err();
        assert_eq!(conflict.code, "REVISION_CONFLICT");
    };

    // Through a second backend, then through the warm writer: nothing commits, and
    // both serve the same revision.
    let other = Storage::new(backend());
    outcomes(&other.open_project(id).unwrap());
    assert_eq!(other.snapshot(id).unwrap(), *warm);
    outcomes(&p);
    let after = resident(&writer, id).unwrap();
    assert_eq!(
        serde_json::to_string(&*after).unwrap(),
        serde_json::to_string(&*warm).unwrap()
    );
}
