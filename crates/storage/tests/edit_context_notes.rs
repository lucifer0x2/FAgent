//! `get_edit_context` reports the project's note count and the configured note limit, the
//! same from node memory, the database read and a plain snapshot.
use fagent_storage::*;
use serde_json::{json, Value};

fn batch(p: &ProjectHandle, revision: u64, key: &str, operations: Value) {
    p.apply_batch(BatchRequest {
        project_id: p.id().into(),
        expected_revision: Some(revision),
        strict: false,
        idempotency_key: key.into(),
        operations: serde_json::from_value(operations).unwrap(),
    })
    .unwrap();
}

#[test]
fn edit_context_reports_note_count_and_limit_on_every_path() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::from(Store::open(temp.path()).unwrap());
    let p = storage.create_project("fixture", "Fixture").unwrap();
    let note = |id: &str| json!({"op":"create_note","id":id,"nodeId":"page","text":id});
    batch(
        &p,
        0,
        "page",
        json!([{"op":"create_node","id":"page","kind":"page","parentId":null,"props":{}}]),
    );
    batch(&p, 1, "notes", json!([note("n1"), note("n2"), note("n3")]));
    let pages = || vec!["page".to_owned()];
    let hits = storage.memory_stats().read_hits;
    let memory = storage.reader_snapshot(p.id()).unwrap().shared;
    assert_eq!(storage.memory_stats().read_hits, hits + 1, "a memory view");
    let database = storage.shared_database(p.id()).unwrap();
    let contexts = [
        memory.edit_context(pages()).unwrap(),
        database.edit_context(pages()).unwrap(),
        edit_context(p.snapshot().unwrap(), pages()).unwrap(),
    ];
    for context in &contexts {
        assert_eq!(context["noteCount"], 3, "{context}");
        assert_eq!(context["noteLimit"], NoteLimits::DEFAULT.notes, "{context}");
    }
    assert_eq!(contexts[0], contexts[1]);
    assert_eq!(contexts[1], contexts[2]);
}
