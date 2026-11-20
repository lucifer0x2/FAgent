use super::*;

#[test]
#[ignore = "metadata-only 200k admission probe; no validation or runtime writes"]
fn two_hundred_thousand_node_indexes_build_without_payload_work() {
    let mut snapshot: Snapshot = serde_json::from_value(json!({
        "schemaVersion":SCHEMA_VERSION,"projectId":"scale","slug":"scale","name":"Scale","revision":1,
        "nodes":[],"tokens":[],"styles":[],"fonts":[],"assets":[],"notes":[],
        "retention":{"oldestCommitRevision":1,"idempotencyPolicy":"kept"}
    })).unwrap();
    for page in 0..20 {
        snapshot.nodes.push(Node {
            id: format!("page_{page}"),
            kind: NodeKind::Page,
            parent_id: None,
            sort_key: page,
            props: json!({}),
        });
    }
    for i in 0..200_000 {
        snapshot.nodes.push(Node {
            id: format!("node_{i}"),
            kind: NodeKind::Rectangle,
            parent_id: Some(format!("page_{}", i % 20)),
            sort_key: i,
            props: json!({"width":10,"height":10,"name":"x".repeat(100)}),
        });
    }
    // The names alone (200k x 100 bytes, about 20 MB) are never encoded or hashed.
    let time = Instant::now();
    let indexes = RevisionIndexes::new(&snapshot.nodes);
    let build = time.elapsed();
    assert!(indexes.dependencies.lock().unwrap().is_none());
    assert_eq!(indexes.page("node_7"), Some("page_7"));
    eprintln!(
        "metadata scale authored={} reserved_dependency_bytes={} build_ms={:.3}",
        snapshot.nodes.len(),
        DEPENDENCY_BUDGET,
        build.as_secs_f64() * 1000.0
    );
}
