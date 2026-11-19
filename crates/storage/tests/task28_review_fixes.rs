//! Storage regressions for the two Task28 review findings, with disposable SQLite.
mod common;
use common::*;
use fagent_storage::*;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

/// Node memory is always on. Each case runs with the project resident, as commits
/// leave it (`warm`), or on a newly opened store whose memory is empty (cold).
fn store(root: &std::path::Path) -> Store {
    Store::open(root).unwrap()
}

fn prepare(root: &Path, store: Store, p: Arc<Project>, warm: bool) -> (Store, Arc<Project>) {
    if warm {
        let storage: Storage = store.clone().into();
        let read = storage.reader_snapshot(p.id()).unwrap();
        assert!(read.from_memory, "commits leave the project resident");
        return (store, p);
    }
    let id = p.id().to_owned();
    drop((p, store));
    let store = self::store(root);
    let stats = Storage::from(store.clone()).memory_stats();
    assert_eq!((stats.memory.loads, stats.resident_bytes), (0, 0), "cold");
    let p = store.open_project(&id).unwrap();
    (store, p)
}

/// After a commit memory serves the committed revision.
fn assert_resident_at(store: &Store, p: &Project, revision: u64) {
    let storage: Storage = store.clone().into();
    let read = storage.reader_snapshot(p.id()).unwrap();
    assert!(read.from_memory);
    assert_eq!(read.shared.snapshot.revision, revision);
}

fn update(props: Value) -> Vec<Operation> {
    vec![Operation::UpdateNode {
        id: "rect".into(),
        props,
        merge: false,
    }]
}

fn scoped(
    p: &Project,
    key: &str,
    pages: Vec<String>,
    strict: bool,
    ops: Vec<Operation>,
) -> ScopedBatchRequest {
    ScopedBatchRequest {
        project_id: p.id().into(),
        expected_revision: Some(1),
        strict,
        idempotency_key: key.into(),
        page_ids: pages,
        operations: ops,
    }
}

#[test]
fn malformed_page_ids_never_commit_or_reserve_replay_keys() {
    for warm in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let store = store(temp.path());
        let p = store.create_project("fixture", "Fixture").unwrap();
        p.apply_batch(request(&p, 0, "seed", initial())).unwrap();
        p.apply_batch(request(&p, 1, "warm", update(json!({"name":"warm"}))))
            .unwrap();
        let (store, p) = prepare(temp.path(), store, p, warm);
        let before = p.snapshot().unwrap();
        let db = rusqlite::Connection::open(temp.path().join("fixture/db.sql")).unwrap();
        let invalid = [
            vec![],
            vec!["page".into(), "page".into()],
            (0..101).map(|i| format!("p{i}")).collect(),
            vec!["bad/id".into()],
            vec![String::new()],
            vec!["p".repeat(129)],
            vec!["page_café".into()],
        ];
        for (i, pages) in invalid.into_iter().enumerate() {
            for strict in [false, true] {
                for fallback in [false, true] {
                    let key = format!("invalid-{i}-{strict}-{fallback}");
                    let ops = if fallback {
                        resource()
                    } else {
                        update(json!({"width":90}))
                    };
                    let req = scoped(&p, &key, pages.clone(), strict, ops);
                    for error in [
                        p.apply_scoped_batch(req.clone()).unwrap_err(),
                        p.dry_run_scoped_batch(req).unwrap_err(),
                    ] {
                        assert_eq!(error.code, "VALIDATION_FAILED", "{error:?}");
                        assert_eq!(error.field.as_deref(), Some("pageIds"));
                    }
                    assert_eq!(p.snapshot().unwrap().revision, before.revision);
                    for table in ["commits", "idempotency"] {
                        let count: i64 = db
                            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                            .unwrap();
                        assert_eq!(count, 2);
                    }
                    assert_eq!(p.snapshot().unwrap().nodes, before.nodes);
                }
            }
        }
        // A rejected payload never reserves the key; valid nonstrict pages have no
        // existence, affected-page or stale dependency semantics.
        let req = scoped(
            &p,
            "invalid-0-false-false",
            (0..100).map(|i| format!("absent_{i}")).collect(),
            false,
            update(json!({"width":90})),
        );
        assert_eq!(
            p.dry_run_scoped_batch(req.clone()).unwrap().base_revision,
            2
        );
        assert_eq!(
            p.apply_scoped_batch(req.clone())
                .unwrap()
                .committed_revision,
            3
        );
        assert_resident_at(&store, &p, 3);
        assert!(p.apply_scoped_batch(req.clone()).unwrap().replayed);
        // Resource operations use the full fallback, with the same nonstrict
        // request shape but no semantic page guard.
        assert_eq!(
            p.apply_scoped_batch(scoped(
                &p,
                "fallback",
                vec!["absent".into()],
                false,
                resource()
            ))
            .unwrap()
            .committed_revision,
            4
        );
        let bundle = p.export_bundle().unwrap();
        let target = tempfile::tempdir().unwrap();
        let restored_store = Store::open(target.path()).unwrap();
        let restored = restored_store
            .restore_bundle("restored", &bundle.bytes)
            .unwrap();
        assert_eq!(
            restored.snapshot().unwrap().nodes,
            p.snapshot().unwrap().nodes
        );
        assert_eq!(restored.snapshot().unwrap().revision, 4);
        assert_eq!(
            restored.snapshot().unwrap().tokens,
            p.snapshot().unwrap().tokens
        );
        assert!(restored.apply_scoped_batch(req).unwrap().replayed);
    }
}

fn resource() -> Vec<Operation> {
    vec![Operation::UpsertToken {
        token: Token {
            id: "unused".into(),
            name: "Unused".into(),
            token_type: "color".into(),
            value: json!("#112233"),
        },
    }]
}

#[test]
fn stale_lww_keeps_latest_conflicts_across_asset_import_and_bundle_restore() {
    for warm in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let store = store(temp.path());
        let p = store.create_project("fixture", "Fixture").unwrap();
        p.apply_batch(request(&p, 0, "seed", initial())).unwrap();
        p.apply_batch(request(
            &p,
            1,
            "first",
            update(json!({"width":120,"height":50})),
        ))
        .unwrap();
        p.import_asset(asset_request(&p, 2, "import")).unwrap();
        p.import_asset(ImportAssetRequest {
            project_id: p.id().into(),
            expected_revision: 3,
            idempotency_key: "font".into(),
            media_type: "font/ttf".into(),
            bytes: include_bytes!("../../../renderer/fonts/NotoSans.ttf").to_vec(),
            font: Some(FontRegistration {
                id: "uploaded".into(),
                family: "Uploaded".into(),
                weight: 400,
                style: "normal".into(),
                license: Some("OFL-1.1".into()),
            }),
        })
        .unwrap();
        p.apply_batch(request(&p, 4, "rewarm", update(json!({"name":"warm"}))))
            .unwrap();
        p.apply_batch(request(&p, 5, "latest", update(json!({"width":140}))))
            .unwrap();
        let (store, p) = prepare(temp.path(), store, p, warm);
        let bundle = p.export_bundle().unwrap();
        let target = tempfile::tempdir().unwrap();
        let restored_store = Store::open(target.path()).unwrap();
        let restored = restored_store
            .restore_bundle("restored", &bundle.bytes)
            .unwrap();
        for source in [&p, &restored] {
            let req = request(source, 1, "stale", update(json!({"width":150,"height":60})));
            let result = source.apply_batch(req.clone()).unwrap();
            assert!(!result.concurrent_unknown, "{result:?}, warm={warm}");
            assert_eq!(result.concurrent_total, Some(2));
            let pairs: Vec<_> = result
                .concurrent
                .unwrap()
                .into_iter()
                .map(|w| (w.node_id, w.key, w.revision))
                .collect();
            assert_eq!(
                pairs,
                vec![
                    ("rect".into(), "height".into(), 2),
                    ("rect".into(), "width".into(), 6)
                ]
            );
            assert_eq!(result.committed_revision, 7);
            if std::ptr::eq(source, &p) {
                assert_resident_at(&store, source, 7);
            }
            assert!(source.apply_batch(req).unwrap().replayed);
        }
    }
}
