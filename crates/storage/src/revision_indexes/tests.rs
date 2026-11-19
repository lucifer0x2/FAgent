use super::*;

pub(super) fn snapshot() -> Snapshot {
    serde_json::from_value(json!({"schemaVersion": SCHEMA_VERSION, "projectId":"p", "slug":"p", "name":"P", "revision":1,
        "nodes":[{"id":"page","parentId":null,"sortKey":1,"kind":"page","props":{}}],
        "tokens":[],"styles":[],"fonts":[],"assets":[],"notes":[],
        "retention":{"oldestCommitRevision":1,"idempotencyPolicy":"kept"}})).unwrap()
}
#[test]
fn lazy_scope_index_is_reused_and_its_full_metadata_fits_admission_reservation() {
    let mut snapshot = snapshot();
    // Frame positions are additional metadata; roots/frames/notes must all fit.
    for i in 0..100 {
        snapshot.nodes.push(Node {
            id: format!("frame{i}"),
            parent_id: Some("page".into()),
            sort_key: i,
            kind: NodeKind::Frame,
            props: json!({"width":10,"height":10}),
        });
    }
    snapshot.notes = (0..5000)
        .map(|i| Note {
            id: format!("note{i}"),
            node_id: "page".into(),
            text: "Intent".into(),
            author: None,
            resolved: false,
            replies: vec![],
        })
        .collect();
    let snapshot = Arc::new(snapshot);
    let indexes = Arc::new(RevisionIndexes::new(&snapshot.nodes));
    let shared = SharedSnapshot::committed(snapshot.clone(), indexes.clone());
    shared.trusted_scoped().unwrap().unwrap();
    let first = indexes.read_scope.lock().unwrap().as_ref().unwrap().clone();
    let again = SharedSnapshot::committed(snapshot, indexes.clone());
    again.trusted_scoped().unwrap().unwrap();
    assert!(Arc::ptr_eq(
        &first,
        indexes.read_scope.lock().unwrap().as_ref().unwrap()
    ));
    assert!(first.estimated_bytes() <= indexes.read_weight);
    let scoped = again.scoped().unwrap();
    assert_eq!(scoped.roots().count(), 1);
    assert_eq!(scoped.frames().count(), 100);
    assert_eq!(scoped.children("page").count(), 100);
    assert_eq!(
        again.scoped().unwrap().notes_for_nodes(["page"]).len(),
        5000
    );
}
