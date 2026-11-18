//! Shared test helpers: a small hand-written project, operation builders, a rebuild
//! comparison that never prints whole indexes, and a deterministic PRNG.
#![allow(dead_code)]
pub mod batch_oracle;
pub mod fixture;
pub mod ops;
pub mod oracle;
pub mod random_cases;
pub mod random_extra;
pub mod random_ops;
pub mod random_props;
pub mod random_resources;
pub mod random_scripted;
pub mod scenario;

#[allow(unused_imports)]
pub use ops::*;

use fagent_node_memory::{IndexDump, ProjectState};
use fagent_scene_input::Snapshot;
use serde_json::{json, Value};

pub fn text(s: &str) -> Value {
    json!({"text":s,"fontId":"font","fontSize":12,"lineHeight":16})
}

/// Library page with masters `m1` (holding nested master `m1n`) and `m2`; page `p1`
/// with root frame `f1` (siblings tie on sort keys) and instance `i1`; page `p2`
/// with root frame `f2`, instance `i2` and a text directly under the page.
pub fn small() -> Snapshot {
    let node = |id: &str, kind: &str, parent: Option<&str>, sort: i64, props: Value| json!({"id":id,"kind":kind,"parentId":parent,"sortKey":sort,"props":props});
    let master = |name: &str| json!({"width":10,"height":10,"component":{"name":name}});
    let frame = json!({"width":100,"height":100});
    let nodes = vec![
        node("lib", "page", None, 0, json!({})),
        node("p1", "page", None, 1, json!({})),
        node("p2", "page", None, 1, json!({})),
        node("m1", "frame", Some("lib"), 0, master("M1")),
        node("m1t", "text", Some("m1"), 0, text("T")),
        node("m1n", "frame", Some("m1"), 1, master("M1N")),
        node("m1nt", "text", Some("m1n"), 0, text("T")),
        node("m2", "frame", Some("lib"), 1, master("M2")),
        node("f1", "frame", Some("p1"), 0, frame.clone()),
        node("a", "rectangle", Some("f1"), 1, json!({})),
        node("b", "rectangle", Some("f1"), 0, json!({})),
        node("c", "text", Some("f1"), 1, text("T")),
        node("g", "group", Some("f1"), 2, json!({})),
        node("gt", "text", Some("g"), 0, text("T")),
        node("i1", "instance", Some("g"), 1, json!({"componentId":"m1"})),
        node("f2", "frame", Some("p2"), 0, frame),
        node("i2", "instance", Some("f2"), 0, json!({"componentId":"m2"})),
        node("t2", "text", Some("p2"), 1, text("T")),
    ];
    snapshot(nodes)
}

pub fn snapshot(nodes: Vec<Value>) -> Snapshot {
    serde_json::from_value(json!({
        "schemaVersion":4,"projectId":"proj","slug":"proj","name":"Proj","revision":1,
        "nodes":nodes,"tokens":[],"styles":[],"assets":[],
        "fonts":[{"id":"font","family":"Noto Sans","weight":400,"style":"normal",
            "assetId":null,"bundledId":"noto","contentHash":"h","license":"OFL",
            "status":"ready"}],
        "retention":{"oldestCommitRevision":0,"idempotencyPolicy":"bounded"}
    }))
    .expect("test snapshot")
}

pub fn load(snapshot: Snapshot) -> ProjectState {
    ProjectState::from_snapshot(snapshot).expect("snapshot loads")
}

/// Child ids of `id` in sibling order.
pub fn kids(state: &ProjectState, id: &str) -> Vec<String> {
    let children = state.children(id).expect("node exists");
    children.map(|n| n.id.clone()).collect()
}

/// `(id, sort_key)` of the children of `id`.
pub fn keyed(state: &ProjectState, id: &str) -> Vec<(String, i64)> {
    let children = state.children(id).expect("node exists");
    children.map(|n| (n.id.clone(), n.sort_key)).collect()
}

/// Everything observable about a project, for exact before/after comparisons.
pub struct Observed {
    pub dump: IndexDump,
    pub snapshot: Snapshot,
    pub bytes: usize,
    pub revision: u64,
}

pub fn observe(state: &ProjectState) -> Observed {
    Observed {
        dump: state.index_dump(),
        snapshot: state.to_snapshot(),
        bytes: state.estimated_bytes(),
        revision: state.revision(),
    }
}

/// Panics with a short message when the state differs from `before`.
pub fn assert_unchanged(state: &ProjectState, before: &Observed) {
    let now = observe(state);
    assert!(now.snapshot == before.snapshot, "snapshot changed");
    // A discarded batch also removes the ids it interned (no view of it survives).
    assert_eq!(now.bytes, before.bytes, "estimated bytes changed");
    assert_eq!(now.revision, before.revision, "revision changed");
    assert_eq!(
        now.dump.stamp_bytes, before.dump.stamp_bytes,
        "stamps changed"
    );
    if let Some(diff) = difference(&now.dump, &before.dump) {
        panic!("index changed: {diff}");
    }
}

/// Panics with a short message when incremental indexes differ from a rebuild.
pub fn assert_rebuild_matches(state: &ProjectState) {
    let rebuilt = load(state.to_snapshot());
    if let Some(diff) = difference(&state.index_dump(), &rebuilt.index_dump()) {
        panic!("incremental index differs from rebuild: {diff}");
    }
}

/// Names the first differing index and key, without printing whole indexes. Write
/// stamps are not compared (a rebuild has none).
pub fn difference(a: &IndexDump, b: &IndexDump) -> Option<String> {
    fn first<V: PartialEq>(
        name: &str,
        a: &std::collections::BTreeMap<String, V>,
        b: &std::collections::BTreeMap<String, V>,
    ) -> Option<String> {
        if a.len() != b.len() {
            return Some(format!("{name}: {} vs {} entries", a.len(), b.len()));
        }
        a.iter()
            .zip(b)
            .find(|(x, y)| x != y)
            .map(|((k, _), (l, _))| format!("{name}: first difference at {k:?}/{l:?}"))
    }
    if a.len != b.len {
        return Some(format!("len {} vs {}", a.len, b.len));
    }
    if a.node_bytes != b.node_bytes {
        return Some(format!("node bytes {} vs {}", a.node_bytes, b.node_bytes));
    }
    first("children", &a.children, &b.children)
        .or_else(|| first("parent", &a.parent, &b.parent))
        .or_else(|| first("page_of", &a.page_of, &b.page_of))
        .or_else(|| first("root_frame_of", &a.root_frame_of, &b.root_frame_of))
        .or_else(|| first("master_of", &a.master_of, &b.master_of))
        .or_else(|| first("instances_of", &a.instances_of, &b.instances_of))
        .or_else(|| first("linked_from", &a.linked_from, &b.linked_from))
        .or_else(|| first("referenced_by", &a.referenced_by, &b.referenced_by))
        .or_else(|| first("notes_on", &a.notes_on, &b.notes_on))
}

/// xorshift64*: deterministic, dependency-free.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    pub fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}
