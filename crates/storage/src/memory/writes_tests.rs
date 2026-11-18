//! The resident project batches and previews stage in: a load of the same
//! incarnation at or past the database revision is never replaced.
use super::{Loaded, Memory};
use crate::{RevisionIndexes, SharedSnapshot, Snapshot, SnapshotCacheIdentity};
use fagent_node_memory::{Begin, Operation};
use serde_json::json;
use std::cell::Cell;
use std::sync::Arc;
use std::time::Duration;

fn snapshot(revision: u64) -> Snapshot {
    let node = |id: &str, kind: &str, parent: Option<&str>| json!({"id":id,"kind":kind,"parentId":parent,"sortKey":0,"props":{}});
    serde_json::from_value(json!({
        "schemaVersion":4,"projectId":"proj","slug":"proj","name":"Proj","revision":revision,
        "nodes":[node("page","page",None), node("frame","frame",Some("page")), node("rect","rectangle",Some("frame"))],
        "tokens":[],"styles":[],"assets":[],"fonts":[],"notes":[],
        "retention":{"oldestCommitRevision":0,"idempotencyPolicy":"bounded"}
    }))
    .unwrap()
}

fn identity(revision: u64) -> SnapshotCacheIdentity {
    SnapshotCacheIdentity {
        project_id: "proj".into(),
        schema_version: 4,
        revision,
        incarnation: "i".into(),
        asset_readiness: vec![],
    }
}

fn rename() -> Vec<Operation> {
    serde_json::from_value(json!([{"op":"update_node","id":"rect","props":{"name":"Renamed"}}]))
        .unwrap()
}

/// Loads `proj` at `revision` as a writer would, counting loads in `loads`.
fn write_at(memory: &Memory, revision: u64, loads: &Cell<usize>) -> fagent_node_memory::HotProject {
    let load = || {
        loads.set(loads.get() + 1);
        Ok(snapshot(revision))
    };
    memory
        .for_write("proj", Some(&identity(revision)), revision, load)
        .unwrap()
}

/// Commits one batch at `revision` and tags the result, as the backends do.
fn commit_at(memory: &Memory, revision: u64) {
    let hot = write_at(memory, revision, &Cell::new(0));
    let Begin::Ready(prepared) = hot.begin(revision, &rename()).unwrap() else {
        panic!("resident at {revision}");
    };
    prepared.commit();
    memory.committed(&hot, Some(&identity(revision + 1)));
}

fn resident_revision(memory: &Memory) -> Option<u64> {
    let project = memory.engine().project("proj")?;
    let revision = project.view().metadata().revision;
    Some(revision)
}

#[test]
fn a_preview_on_an_older_snapshot_keeps_the_newer_resident_copy() {
    let memory = Memory::default();
    let loads = Cell::new(0);
    write_at(&memory, 3, &loads);
    commit_at(&memory, 3);
    let load_id = memory.engine().resident("proj");
    // The preview's repeatable-read snapshot predates the commit.
    let preview = memory
        .for_preview("proj", &identity(3), 3, || panic!("no load past memory"))
        .unwrap();
    assert!(preview.is_none());
    assert_eq!(memory.engine().resident("proj"), load_id);
    assert_eq!(resident_revision(&memory), Some(4));
    // A private load serves it without touching the resident copy.
    let detached = memory.detached(|| Ok(snapshot(3))).unwrap();
    assert!(matches!(
        detached.begin(3, &rename()).unwrap(),
        Begin::Ready(_)
    ));
    assert_eq!(memory.engine().resident("proj"), load_id);
    // At the current revision the preview stages in the resident copy.
    let current = memory
        .for_preview("proj", &identity(4), 4, || panic!("resident"))
        .unwrap()
        .unwrap();
    assert_eq!(Some(current.load_id()), load_id);
    assert_eq!(loads.get(), 1);
}

#[test]
fn a_write_at_the_resident_revision_never_reloads() {
    let memory = Memory::default();
    let loads = Cell::new(0);
    let first = write_at(&memory, 3, &loads);
    let again = write_at(&memory, 3, &loads);
    assert_eq!(first.load_id(), again.load_id());
    assert_eq!(loads.get(), 1);
    let stats = memory.stats();
    assert_eq!((stats.write_loads, stats.write_hits), (1, 1));
}

/// PostgreSQL releases the row lock at `tx.commit`, before the batch publishes: the
/// next writer reads the new revision and must wait for that publish rather than
/// reload.
#[test]
fn a_writer_racing_a_publish_waits_for_it_instead_of_reloading() {
    let memory = Memory::default();
    let hot = write_at(&memory, 3, &Cell::new(0));
    let Begin::Ready(prepared) = hot.begin(3, &rename()).unwrap() else {
        panic!("resident at 3");
    };
    let next = std::thread::scope(|scope| {
        let writer = scope.spawn(|| {
            let loads = Cell::new(0);
            let project = write_at(&memory, 4, &loads);
            (project.load_id(), loads.get())
        });
        std::thread::sleep(Duration::from_millis(50));
        prepared.commit();
        memory.committed(&hot, Some(&identity(4)));
        writer.join().unwrap()
    });
    assert_eq!(next, (hot.load_id(), 0));
    assert_eq!(resident_revision(&memory), Some(4));
}

/// A read that falls back to the database while a writer loads the same revision
/// keeps the writer's copy.
#[test]
fn a_read_never_replaces_a_resident_copy_at_its_revision() {
    let memory = Memory::default();
    let load_id = Cell::new(None);
    let load = || -> crate::Result<Loaded> {
        let hot = write_at(&memory, 3, &Cell::new(0));
        load_id.set(Some(hot.load_id()));
        let snapshot = snapshot(3);
        let indexes = Arc::new(RevisionIndexes::new(&snapshot.nodes));
        let shared = SharedSnapshot::committed(Arc::new(snapshot), indexes);
        Ok((shared, Some(identity(3))))
    };
    let read = memory
        .scoped_read("frame", || Ok(Some(identity(3))), load)
        .unwrap();
    assert!(!read.from_memory);
    assert_eq!(memory.engine().resident("proj"), load_id.get());
    let stats = memory.stats();
    assert_eq!((stats.read_loads, stats.memory.evictions), (0, 0));
    // The same holds for a committed copy past the read's snapshot.
    commit_at(&memory, 3);
    let committed = memory.engine().resident("proj");
    let stale = || -> crate::Result<Loaded> {
        let snapshot = snapshot(3);
        let indexes = Arc::new(RevisionIndexes::new(&snapshot.nodes));
        Ok((
            SharedSnapshot::committed(Arc::new(snapshot), indexes),
            Some(identity(3)),
        ))
    };
    memory
        .scoped_read("frame", || Ok(Some(identity(3))), stale)
        .unwrap();
    assert_eq!(memory.engine().resident("proj"), committed);
}
