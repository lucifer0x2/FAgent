mod postgres_support;
use fagent_storage::*;
use postgres_support::*;

struct RestoreNamespace(String);
impl Drop for RestoreNamespace {
    fn drop(&mut self) {
        let _ = sql().execute(
            "UPDATE fagent_runtime_v1.schema_registry SET namespace=$1 WHERE singleton",
            &[&self.0],
        );
    }
}

#[test]
#[ignore = "requires fresh PostgreSQL via scripts/task19-check.sh"]
fn authoritative_namespace_rotation_invalidates_warm_memory() {
    let backend = backend();
    let _restore = RestoreNamespace(backend.database_namespace().to_owned());
    let storage = Storage::new(backend);
    let p = project(&storage, "namespace");
    p.apply_batch(request(p.id(), 0, "seed", page("Warm")))
        .unwrap();
    let expected = p.snapshot().unwrap();
    let replacement = uuid::Uuid::new_v4().to_string();
    sql()
        .execute(
            "UPDATE fagent_runtime_v1.schema_registry SET namespace=$1 WHERE singleton",
            &[&replacement],
        )
        .unwrap();
    let mut control = sql();
    let lock = lock_entity_tables(&mut control);
    // Resident at the same revision, but of the previous namespace.
    assert_eq!(p.snapshot().unwrap_err().code, "STORAGE_BUSY");
    lock.rollback().unwrap();
    assert_eq!(p.snapshot().unwrap(), expected);
    let lock = lock_entity_tables(&mut control);
    assert_eq!(p.snapshot().unwrap(), expected);
    lock.rollback().unwrap();
    let reconnected = PostgresBackend::with_config(config()).unwrap();
    assert_eq!(reconnected.database_namespace(), replacement);
    let new_storage = Storage::new(reconnected);
    assert_eq!(new_storage.snapshot(p.id()).unwrap(), expected);
    let lock = lock_entity_tables(&mut control);
    assert_eq!(new_storage.snapshot(p.id()).unwrap(), expected);
    lock.rollback().unwrap();
}
