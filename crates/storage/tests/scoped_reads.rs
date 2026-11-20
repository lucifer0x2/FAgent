mod postgres_support;
mod scoped_support;
use fagent_storage::{read_scope::*, *};
use scoped_support::*;
use serde_json::json;
use std::sync::Arc;

/// Shared with scene-input: the independent full-expansion closure reference.
#[path = "../../scene-input/tests/scoped_reference/mod.rs"]
mod scoped_reference;

const SCOPES: [(ReadScope<'static>, bool); 6] = [
    (ReadScope::Page("p1"), true),
    (ReadScope::Page("p1"), false),
    (ReadScope::Target("f1"), false),
    (ReadScope::Target("instance"), true),
    (ReadScope::Target("instance/nested/leaf_box"), false),
    (ReadScope::Page("library"), false),
];

fn parity(shared: &SharedSnapshot) -> Vec<ScopeStats> {
    let scoped = shared
        .trusted_scoped()
        .unwrap()
        .expect("builtin committed source");
    let all = resolved_expanded_nodes(&shared.snapshot, "night").unwrap();
    SCOPES
        .into_iter()
        .map(|(scope, definitions)| {
            let mut nodes = scoped.expand(scope, definitions).unwrap();
            let actual = scoped.resolve(&mut nodes, "night").unwrap();
            let ids: Vec<_> = actual.iter().map(|n| n.id.clone()).collect();
            let reference = scoped_reference::reference_ids(&shared.snapshot, scope, definitions);
            assert_eq!(ids, reference, "{scope:?} definitions={definitions}");
            let expected: Vec<_> = all
                .iter()
                .filter(|n| ids.contains(&n.id))
                .cloned()
                .collect();
            assert_eq!(actual, expected);
            nodes.stats
        })
        .collect()
}

/// Every scope's resolved nodes, from a database or memory snapshot.
fn resolved(shared: &SharedSnapshot) -> Vec<Vec<Node>> {
    let scoped = shared.trusted_scoped().unwrap().expect("builtin source");
    SCOPES
        .into_iter()
        .map(|(scope, definitions)| {
            let mut nodes = scoped.expand(scope, definitions).unwrap();
            scoped.resolve(&mut nodes, "night").unwrap()
        })
        .collect()
}

fn exercise(storage: &Storage, p: &ProjectHandle) {
    seed(p);
    let old = storage.shared_database(p.id()).unwrap();
    let original = parity(&old);
    let again = storage.shared_database(p.id()).unwrap();
    assert_eq!(parity(&again), original);
    strict(
        p,
        "reorder",
        vec![Operation::ReorderNode {
            id: "instance".into(),
            index: 0,
        }],
    );
    let reordered = storage.shared_database(p.id()).unwrap();
    parity(&reordered);
    // Property-only candidate indexes are built in BTreeMap ID order; the read
    // index must instead bind to the backend's actual sorted snapshot vector.
    strict(p, "sparse", vec![update("r1", json!({"width":21}))]);
    let next = storage.shared_database(p.id()).unwrap();
    parity(&next);
    // Node memory is always on: the commit left the project resident, and its view
    // resolves every scope as the database snapshot does.
    let resident = storage.scoped_snapshot(p.id(), "r1").unwrap();
    assert!(resident.from_memory);
    assert_eq!(resident.shared.snapshot.revision, next.snapshot.revision);
    assert_eq!(resolved(&resident.shared), resolved(&next));
    assert_eq!(parity(&old), original);
    let mut swapped = storage.shared_database(p.id()).unwrap();
    swapped.snapshot = Arc::new((*swapped.snapshot).clone());
    assert!(swapped.trusted_scoped().unwrap().is_none());
    swapped.snapshot = Arc::new({
        let mut invalid = (*swapped.snapshot).clone();
        invalid
            .nodes
            .iter_mut()
            .find(|n| n.id == "r2")
            .unwrap()
            .props["bad"] = json!(true);
        invalid
    });
    assert_eq!(swapped.scoped().err().unwrap().code, "VALIDATION_FAILED");
    let mut swapped_index = storage.shared_database(p.id()).unwrap();
    swapped_index.indexes = old.indexes.clone();
    assert!(swapped_index.trusted_scoped().unwrap().is_none());
}

#[test]
fn sqlite_scope_pairing_warm_commit_order_old_readers_and_field_swap() {
    let (temp, storage, p) = sqlite("scoped-reads");
    exercise(&storage, &p);
    let expected = parity(&storage.shared_database(p.id()).unwrap());
    let project_id = p.id().to_string();
    drop(p);
    drop(storage);
    let fresh: Storage = Store::open(temp.path()).unwrap().into();
    assert_eq!(
        expected,
        parity(&fresh.shared_database(&project_id).unwrap())
    );
}

#[test]
#[ignore = "fresh disposable PostgreSQL via scripts/task19-check.sh"]
fn postgres_scope_pairing_warm_commit_order_old_readers_and_field_swap() {
    let backend = postgres_support::backend();
    let storage = Storage::new(backend);
    let p = postgres_support::project(&storage, "scoped-reads");
    exercise(&storage, &p);
    let (_, cold) = postgres_support::setup();
    assert_eq!(
        parity(&storage.shared_database(p.id()).unwrap()),
        parity(&cold.shared_database(p.id()).unwrap())
    );
}
