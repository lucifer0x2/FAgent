//! Differential runs on SQLite: the committed scoped-tree fixture (always) and a
//! generated 5k-node project (needs Node.js), plus the 200k timing.
use super::fixture::{generate, sqlite_pair};
use super::queries::{edges, edits, inner, roots, Caps, Targets};
use super::{timing, Pair};
use fagent_storage::{node_memory_shadow_stats, NodeKind, Snapshot};
use serde_json::{json, Value};

async fn run(pair: &mut Pair, snapshot: &Snapshot, caps: Caps, enrich: Vec<Value>) {
    let mut revision = pair.off.store.snapshot(&pair.project).unwrap().revision;
    for (i, ops) in enrich.into_iter().enumerate() {
        revision = pair.commit(revision, &format!("enrich-{i}"), ops).await;
    }
    let targets = Targets::of(snapshot, &caps);
    let virtuals = roots(pair, &targets, caps.virtuals).await;
    inner(pair, &targets, &virtuals).await;
    edges(pair, &targets, revision).await;
    let first = targets.roots.first().cloned().unwrap_or_default();
    for (i, ops) in edits(&targets).into_iter().enumerate() {
        revision = pair.commit(revision, &format!("edit-{i}"), ops).await;
        pair.one("get_design_context", json!({"frameId": first}))
            .await;
        let leaf = targets.leaves.first().cloned().unwrap_or_default();
        pair.one("get_subtree", json!({"nodeId": leaf})).await;
    }
    roots(pair, &targets, 0).await;
}

/// Links between root frames, notes on frames and master content, a nested
/// instance inside a master and a token, as the I0 slice check used.
fn enrich(snapshot: &Snapshot, targets: &Targets) -> Vec<Value> {
    let masters: Vec<&str> = snapshot
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Frame && n.props.get("component").is_some())
        .map(|n| n.id.as_str())
        .collect();
    let first_child = |parent: &str| {
        let child = snapshot
            .nodes
            .iter()
            .find(|n| n.parent_id.as_deref() == Some(parent));
        child.map(|n| n.id.clone()).unwrap_or(parent.to_owned())
    };
    let r = &targets.plain;
    let (a, b) = (first_child(&r[0]), first_child(&r[2 % r.len()]));
    let mut ops = vec![
        json!({"op":"update_node","id":a,"props":{"links":[{"to":r[1 % r.len()]}]}}),
        json!({"op":"update_node","id":b,"props":{"links":[{"to":r[0]}]}}),
        json!({"op":"create_note","id":"note_root","nodeId":r[0],"text":"check"}),
        json!({"op":"upsert_token","token":{"id":"brand","name":"Brand","type":"color","value":"#ff0000"}}),
    ];
    if let [m0, m1, ..] = masters[..] {
        ops.push(
            json!({"op":"create_note","id":"note_master","nodeId":first_child(m0),"text":"check"}),
        );
        ops.push(
            json!({"op":"create_node","id":"nested_0","kind":"instance","parentId":m1,
            "props":{"componentId":m0,"name":"Nested"}}),
        );
    }
    vec![Value::Array(ops)]
}

#[tokio::test(flavor = "multi_thread")]
async fn committed_fixture_reads_from_memory_match_today() {
    let (_dir, storage, _) = crate::reads::scoped_tree_fixture::fixture();
    let snapshot = storage.snapshot("scoped-tree").unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (on, off, id) = sqlite_pair(temp.path(), "fixture", &snapshot);
    let mut pair = Pair::new(on, off, &id);
    let start = node_memory_shadow_stats();
    let caps = Caps {
        nested: 50,
        instances: 50,
        virtuals: 50,
    };
    run(&mut pair, &snapshot, caps, vec![]).await;
    for id in [
        "placed/nested",
        "placed/nested/copy",
        "placed/box",
        "outside/box",
    ] {
        pair.one("get_subtree", json!({"nodeId": id})).await;
        pair.walk("get_outline", json!({"nodeId": id, "limit": 1}))
            .await;
        pair.one("find_nodes", json!({"nodeId": id, "kinds": ["text"]}))
            .await;
    }
    pair.assert_identical("fixture", start);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Task28 I2 differential on a generated 5k project; needs Node.js"]
async fn generated_5k_reads_from_memory_match_today() {
    let temp = tempfile::tempdir().unwrap();
    let snapshot = generate(5_000, temp.path());
    let (on, off, id) = sqlite_pair(temp.path(), "gen5k", &snapshot);
    let mut pair = Pair::new(on, off, &id);
    let start = node_memory_shadow_stats();
    let caps = Caps {
        nested: 60,
        instances: 60,
        virtuals: 120,
    };
    let targets = Targets::of(&snapshot, &caps);
    let enrichment = enrich(&snapshot, &targets);
    run(&mut pair, &snapshot, caps, enrichment).await;
    pair.assert_identical("gen5k", start);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Task28 I2 timing on a generated 200k project; release build, needs Node.js"]
async fn generated_200k_frame_context_timing() {
    let nodes = std::env::var("MEMORY_DIFF_NODES").map_or(200_000, |v| v.parse().unwrap());
    let temp = tempfile::tempdir().unwrap();
    let snapshot = generate(nodes, temp.path());
    let (on, off, id) = sqlite_pair(temp.path(), "gen200k", &snapshot);
    let targets = Targets::of(
        &snapshot,
        &Caps {
            nested: 0,
            instances: 0,
            virtuals: 0,
        },
    );
    let frame = timing::normal_root(&snapshot, &targets.roots);
    let mut pair = Pair::new(on, off, &id);
    let mut report = timing::frame_context(&mut pair, &frame).await;
    report["afterWrite"] = timing::after_writes(&mut pair, &frame, false).await;
    println!(
        "memory-timing sqlite {}",
        json!({"authoredNodes": snapshot.nodes.len(), "timing": report})
    );
    assert!(pair.differences.is_empty(), "{:?}", pair.differences);
}
