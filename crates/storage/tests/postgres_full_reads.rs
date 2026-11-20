//! Latest-revision full snapshots on PostgreSQL: served from node memory when it holds
//! the revision and incarnation the database reports, otherwise loaded from SQL.
mod postgres_support;
use fagent_storage::*;
use postgres_support::*;
use std::sync::Barrier;

#[test]
#[ignore = "requires fresh PostgreSQL via scripts/task19-check.sh"]
fn hot_read_uses_canonical_identity_and_skips_locked_entity_tables() {
    let storage = Storage::new(backend());
    let p = project(&storage, "hot");
    let mut seed = page("Cold\0then hot");
    seed.extend(serde_json::from_value::<Vec<Operation>>(serde_json::json!([
        {"op":"upsert_token","token":{"id":"zero","name":"Number\0name","type":"number","value":-0.0}}
    ])).unwrap());
    p.apply_batch(request(p.id(), 0, "seed", seed)).unwrap();
    let slug = storage.project_meta(p.id()).unwrap().slug;
    let cold = storage.database_snapshot(&slug).unwrap().0;
    let mut control = sql();
    let lock = lock_entity_tables(&mut control);
    // The commit left the project resident: by slug or ID, no entity read.
    let hot = storage.snapshot(&slug).unwrap();
    assert_eq!(hot, *cold);
    assert_eq!(storage.snapshot(p.id()).unwrap(), hot);
    assert_eq!(
        serde_json::to_string(&hot).unwrap(),
        serde_json::to_string(&*cold).unwrap()
    );
    assert_eq!(hot.nodes[0].props["name"], "Cold\0then hot");
    assert_eq!(
        hot.tokens[0].value.as_f64().unwrap().to_bits(),
        (-0.0_f64).to_bits()
    );
    let empty = backend();
    let error = empty.snapshot(p.id()).unwrap_err();
    assert_eq!(error.code, "STORAGE_BUSY");
    lock.rollback().unwrap();
    assert_eq!(empty.snapshot(p.id()).unwrap(), *cold);
}

#[test]
#[ignore = "requires fresh PostgreSQL via scripts/task19-check.sh"]
fn full_reads_follow_another_backends_commit() {
    let storage = Storage::new(backend());
    let p = project(&storage, "full-read-race");
    p.apply_batch(request(p.id(), 0, "seed", page("Old")))
        .unwrap();
    let old = p.snapshot().unwrap();

    let other = backend();
    assert_eq!(other.snapshot(p.id()).unwrap(), old);
    let write = other
        .apply_batch(p.id(), request(p.id(), 1, "new", rename("New")))
        .unwrap();
    assert_eq!(write.committed_revision, 2);

    let mut control = sql();
    let lock = lock_entity_tables(&mut control);
    // `p`'s memory is behind the database: its next full read needs SQL.
    assert_eq!(p.snapshot().unwrap_err().code, "STORAGE_BUSY");
    // The writer's commit left its memory at the new revision.
    let written = other.snapshot(p.id()).unwrap();
    lock.rollback().unwrap();
    assert_eq!(written.revision, 2);
    assert_eq!(written.nodes[0].props["name"], "New");

    let fresh = backend();
    let barrier = Barrier::new(4);
    let id = p.id();
    let reads = std::thread::scope(|scope| {
        let independent = scope.spawn(|| {
            barrier.wait();
            other.snapshot(id).unwrap()
        });
        let fresh = scope.spawn(|| {
            barrier.wait();
            fresh.snapshot(id).unwrap()
        });
        let original = scope.spawn(|| {
            barrier.wait();
            p.snapshot().unwrap()
        });
        barrier.wait();
        [
            independent.join().unwrap(),
            fresh.join().unwrap(),
            original.join().unwrap(),
        ]
    });
    let newest = p.snapshot().unwrap();
    assert_eq!(newest.revision, 2);
    assert_eq!(newest.nodes[0].props["name"], "New");
    assert!(reads.iter().all(|snapshot| *snapshot == newest));
}

#[test]
#[ignore = "requires fresh PostgreSQL via scripts/task19-check.sh"]
fn project_deletion_and_recreation_block_a_warm_previous_generation() {
    let storage = Storage::new(backend());
    let p = project(&storage, "generation");
    let old = p.snapshot().unwrap();
    let mut control = sql();
    control
        .execute(
            "DELETE FROM fagent_runtime_v1.projects WHERE project_id=$1",
            &[&p.id()],
        )
        .unwrap();
    assert_eq!(p.snapshot().unwrap_err().code, "PROJECT_NOT_FOUND");
    control.execute("INSERT INTO fagent_runtime_v1.projects(project_id,slug,name,schema_version,revision) VALUES($1,$2,$3,$4,0)", &[&p.id(), &old.slug, &b"Replacement".to_vec(), &(SCHEMA_VERSION as i32)]).unwrap();
    let lock = lock_entity_tables(&mut control);
    // Same ID and revision, new generation: the resident copy is not served.
    assert_eq!(p.snapshot().unwrap_err().code, "STORAGE_BUSY");
    lock.rollback().unwrap();
    let new = p.snapshot().unwrap();
    assert_eq!(new.project_id, old.project_id);
    assert_eq!(new.revision, old.revision);
    assert_eq!(new.name, "Replacement");
    let lock = lock_entity_tables(&mut control);
    assert_eq!(p.snapshot().unwrap(), new);
    lock.rollback().unwrap();
}
