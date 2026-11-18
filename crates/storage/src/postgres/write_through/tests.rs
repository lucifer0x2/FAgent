use super::*;
use crate::Snapshot;
use serde_json::json;
use std::sync::Arc;

fn snapshot() -> Snapshot {
    serde_json::from_value(json!({
        "schemaVersion": crate::SCHEMA_VERSION, "projectId": "p", "slug": "p", "name": "P",
        "revision": 4, "nodes": [], "tokens": [], "styles": [], "notes": [], "assets": [],
        "fonts": [], "retention": {"oldestCommitRevision": 1, "idempotencyPolicy": "kept"}
    }))
    .unwrap()
}

#[test]
fn evicts_only_from_a_cache_bound_to_the_observed_namespace() {
    let cache = SnapshotCache::new("db");
    cache.put(&Arc::new(snapshot()), 7);
    Committed::new("p".into(), "other".into()).evict(Some(&cache));
    assert!(cache.get("p", 7, 4).is_some());
    Committed::new("p".into(), "db".into()).evict(Some(&cache));
    assert!(cache.get("p", 7, 4).is_none());
    Committed::new("p".into(), "db".into()).evict(None);
}
