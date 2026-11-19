//! Rows written behind the validator (older releases, raw SQL) must make trusted
//! scoped reads and commits fail with errors, never panics. Temporary roots only.
mod scoped_support;
use fagent_storage::{read_scope::*, *};
use scoped_support::*;
use serde_json::json;

/// A seeded temporary SQLite project whose `id` row gets `props_json` directly.
fn corrupt(id: &str, props: &str) -> (tempfile::TempDir, Storage, String) {
    let (temp, storage, p) = sqlite("corrupt");
    seed(&p);
    let project = p.id().to_string();
    drop(p);
    drop(storage);
    let db = rusqlite::Connection::open(temp.path().join("corrupt/db.sql")).unwrap();
    let rows = db
        .execute(
            "UPDATE nodes SET props_json=?1 WHERE id=?2",
            rusqlite::params![props, id],
        )
        .unwrap();
    assert_eq!(rows, 1);
    drop(db);
    let storage: Storage = Store::open(temp.path()).unwrap().into();
    (temp, storage, project)
}

fn options(frame: &str) -> SceneOptions {
    SceneOptions {
        frame_id: frame.into(),
        width: 400.,
        height: 400.,
        theme: "default".into(),
        state: "default".into(),
    }
}

fn reads_fail(storage: &Storage, project: &str, field: &str) {
    let shared = storage.shared_database(project).unwrap();
    let scoped = shared.trusted_scoped().unwrap().expect("builtin authority");
    for (scope, definitions) in [
        (ReadScope::Target("f1"), true),
        (ReadScope::Target("f1"), false),
        (ReadScope::Page("p1"), false),
    ] {
        let error = scoped
            .expand(scope, definitions)
            .and_then(|mut nodes| scoped.resolve(&mut nodes, "default"))
            .unwrap_err();
        assert_eq!(error.field.as_deref(), Some(field), "{scope:?} {error:?}");
    }
    let error = scoped.prepare_input_with_scale(&options("f1"), 1.).err();
    assert_eq!(error.unwrap().field.as_deref(), Some(field));
    // The unrelated page keeps the trusted path's bounded behavior.
    assert!(scoped.prepare_input_with_scale(&options("f2"), 1.).is_ok());
}

fn commit_fails(storage: &Storage, project: &str, id: &str) {
    let p = storage.open_project(project).unwrap();
    let request = BatchRequest {
        project_id: project.into(),
        expected_revision: Some(p.snapshot().unwrap().revision),
        strict: false,
        idempotency_key: "after-corruption".into(),
        operations: vec![update(id, json!({"name":"touched"}))],
    };
    let error = p.apply_batch(request).unwrap_err();
    assert_eq!(error.code, "VALIDATION_FAILED", "{error:?}");
}

#[test]
fn instance_with_non_object_props_fails_reads_and_commits() {
    let (_temp, storage, project) = corrupt("instance", r#"["not","an","object"]"#);
    reads_fail(&storage, &project, "props.componentId");
    commit_fails(&storage, &project, "instance");
}

/// Batches are judged on their delta: a stored defect is reported only when the batch
/// reaches it (here, touching the instance), not by an unrelated write.
#[test]
fn instance_with_missing_master_fails_reads_and_commits() {
    let (_temp, storage, project) = corrupt("instance", r#"{"componentId":"missing"}"#);
    reads_fail(&storage, &project, "props.componentId");
    commit_fails(&storage, &project, "instance");
}

#[test]
fn authored_node_with_scalar_props_fails_resolution_and_commits() {
    let (_temp, storage, project) = corrupt("r1", "5");
    reads_fail(&storage, &project, "props");
    commit_fails(&storage, &project, "r1");
}

/// A corrupt node does not lock its project: an unrelated write commits, and so does
/// a strict delete of the corrupt node itself (the repair path).
#[test]
fn scalar_props_leave_unrelated_writes_and_the_repair_delete_open() {
    let (_temp, storage, project) = corrupt("r1", "5");
    let p = storage.open_project(&project).unwrap();
    for (key, strict, operations) in [
        (
            "unrelated",
            false,
            vec![update("r2", json!({"name":"touched"}))],
        ),
        (
            "repair",
            true,
            vec![Operation::DeleteSubtree { id: "r1".into() }],
        ),
    ] {
        let request = BatchRequest {
            project_id: project.clone(),
            expected_revision: Some(p.snapshot().unwrap().revision),
            strict,
            idempotency_key: key.into(),
            operations,
        };
        p.apply_batch(request)
            .unwrap_or_else(|e| panic!("{key}: {e:?}"));
    }
    assert!(p.snapshot().unwrap().nodes.iter().all(|n| n.id != "r1"));
}
