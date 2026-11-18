//! Real-data differential and timing on a disposable restore of a native backup
//! (never the live cluster). Both states share the restored database, each with its
//! own node memory and snapshot cache as in production; `off` reads the database
//! snapshot only. Commits alternate between the two, so `on` also sees writes of
//! another writer and must catch up through the freshness probe.
//!
//! `MEMORY_DIFF_REALDATA_URL='host=127.0.0.1 port=<port> dbname=<db>' cargo test \
//!   --release -p fagent-server --lib memory_diff::realdata -- --ignored --nocapture`
use super::queries::{edges, edits, inner, roots, Caps, Targets};
use super::{timing, Pair};
use fagent_storage::{PostgresBackend, PostgresConfig, SnapshotCache, Storage};
use serde_json::json;

pub(super) fn storage(url: &str) -> Storage {
    let config = PostgresConfig::new(url);
    let backend = PostgresBackend::with_config(config).unwrap();
    let cache = SnapshotCache::new(backend.cache_namespace());
    Storage::new(backend.with_cache(cache).unwrap())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "real-data check against a disposable restore; see the module docs"]
async fn real_projects_read_from_memory_match_today() {
    let url = std::env::var("MEMORY_DIFF_REALDATA_URL").expect("MEMORY_DIFF_REALDATA_URL");
    assert!(!url.contains("2703"), "never the live cluster");
    let (on, off) = (storage(&url), storage(&url));
    let start = on.memory_stats();
    let projects = off.list_projects().unwrap();
    let mut total = Pair::new(on.clone(), off.clone(), "");
    for (i, meta) in projects.iter().enumerate() {
        let mut pair = Pair::new(on.clone(), off.clone(), &meta.project_id);
        let snapshot = pair.off.store.snapshot(&meta.project_id).unwrap();
        let caps = Caps {
            nested: 80,
            instances: 80,
            virtuals: 150,
        };
        let targets = Targets::of(&snapshot, &caps);
        let Some(frame) =
            (!targets.plain.is_empty()).then(|| timing::normal_root(&snapshot, &targets.plain))
        else {
            continue;
        };
        // Timing first, while both states are cold.
        let mut time = timing::frame_context(&mut pair, &frame).await;
        time["afterWrite"] = timing::after_writes(&mut pair, &frame, true).await;
        let id = meta.project_id.clone();
        let mut revision = pair
            .off
            .db(move |s| s.project_meta(&id))
            .await
            .unwrap()
            .revision;
        let virtuals = roots(&mut pair, &targets, caps.virtuals).await;
        inner(&mut pair, &targets, &virtuals).await;
        edges(&mut pair, &targets, revision).await;
        for (step, ops) in edits(&targets).into_iter().enumerate() {
            pair.shared = Some(step % 2 == 0);
            revision = pair
                .commit(revision, &format!("memory-diff-{step}"), ops)
                .await;
            pair.one("get_design_context", json!({"frameId": targets.plain[0]}))
                .await;
            pair.walk("get_outline", json!({"nodeId": frame, "limit": 50}))
                .await;
        }
        roots(&mut pair, &targets, 0).await;
        println!(
            "memory-realdata {}",
            json!({"project": i, "authoredNodes": snapshot.nodes.len(),
                "roots": targets.roots.len(), "virtuals": virtuals.len(),
                "compared": pair.compared, "differences": pair.differences, "timing": time})
        );
        total.compared += pair.compared;
        total.differences.extend(pair.differences);
    }
    total.assert_identical("realdata", start);
}
