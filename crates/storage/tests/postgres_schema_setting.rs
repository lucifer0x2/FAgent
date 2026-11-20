mod postgres_support;
use fagent_storage::*;
use postgres_support::*;

/// Drops the test's own schema even when an assertion fails.
struct DropSchema(String);
impl Drop for DropSchema {
    fn drop(&mut self) {
        let _ = sql().batch_execute(&format!("DROP SCHEMA IF EXISTS \"{}\" CASCADE", self.0));
    }
}

#[test]
#[ignore = "requires fresh PostgreSQL via scripts/task19-check.sh"]
fn a_configured_schema_holds_every_runtime_table_and_project() {
    let name = format!("fagent_schema_{}", uuid::Uuid::new_v4().simple());
    let _cleanup = DropSchema(name.clone());
    let mut settings = config();
    settings.schema = DatabaseSchema::parse(&name).unwrap();
    let backend = PostgresBackend::with_config(settings.clone()).unwrap();
    let namespace = backend.database_namespace().to_owned();
    let storage = Storage::new(backend);
    let p = project(&storage, "schema-setting");
    p.apply_batch(request(p.id(), 0, "seed", page("Home")))
        .unwrap();

    let mut client = sql();
    let tables: Vec<String> = client
        .query(
            "SELECT table_name::text FROM information_schema.tables WHERE table_schema=$1 ORDER BY 1",
            &[&name],
        )
        .unwrap()
        .iter()
        .map(|row| row.get(0))
        .collect();
    for table in [
        "binaries",
        "commits",
        "entities",
        "idempotency",
        "projects",
        "schema_registry",
        "viewer_credentials",
        "viewer_sessions",
    ] {
        assert!(
            tables.iter().any(|t| t == table),
            "{table} missing from {tables:?}"
        );
    }
    let stored: i64 = client
        .query_one(
            &format!("SELECT count(*) FROM \"{name}\".projects WHERE project_id=$1"),
            &[&p.id()],
        )
        .unwrap()
        .get(0);
    assert_eq!(stored, 1);
    let elsewhere: i64 = client
        .query_one(
            "SELECT count(*) FROM information_schema.tables
             WHERE table_name='projects' AND table_schema NOT IN ($1, $2)",
            &[&name, &DEFAULT_DATABASE_SCHEMA],
        )
        .unwrap()
        .get(0);
    assert_eq!(
        elsewhere, 0,
        "runtime tables created outside the configured schema"
    );

    // Reconnecting to the same schema keeps its registry and data.
    let reopened = PostgresBackend::with_config(settings).unwrap();
    assert_eq!(reopened.database_namespace(), namespace);
    let reopened = Storage::new(reopened);
    assert_eq!(reopened.snapshot(p.id()).unwrap(), p.snapshot().unwrap());
}
