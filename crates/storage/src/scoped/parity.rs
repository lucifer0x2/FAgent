//! Closures and footprints against the frozen reference algorithms: the snapshot
//! dependency index and the engine's views (`edit_context`, page guards), the
//! engine's commit footprint and the full-scan footprint of resource imports.
use super::{dependency_index::DependencyIndex, history, reference_history};
use crate::*;
use fagent_node_memory::{Begin, MemoryConfig, NodeMemory, SystemClock};
#[path = "parity_notes_pages.rs"]
mod notes_pages;

pub(crate) fn seed() -> Snapshot {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(temp.path()).unwrap();
    let p = store.create_project("parity", "Parity").unwrap();
    p.apply_batch(BatchRequest { project_id: p.id().into(), expected_revision: Some(0), strict: false, idempotency_key: "seed".into(), operations: serde_json::from_value(json!([
        {"op":"upsert_token","token":{"id":"base","name":"Base","type":"color","value":"#112233"}},
        {"op":"upsert_token","token":{"id":"alias","name":"Alias","type":"color","value":{"default":{"aliasId":"base"},"themes":{"night":"#ffffff"}}}},
        {"op":"upsert_style","style":{"id":"fill","name":"Fill","kind":"fill","value":{"tokenId":"alias"}}},
        {"op":"upsert_style","style":{"id":"text-style","name":"Text","kind":"text","value":{"fontId":"font_bundled"}}},
        {"op":"create_node","id":"p1","kind":"page","parentId":null,"props":{"name":"One"}},
        {"op":"create_node","id":"p2","kind":"page","parentId":null,"props":{"name":"Two"}},
        {"op":"create_node","id":"library","kind":"page","parentId":null,"props":{"name":"Library"}},
        {"op":"create_node","id":"empty","kind":"page","parentId":null,"props":{"name":"Empty"}},
        {"op":"create_node","id":"f1","kind":"frame","parentId":"p1","props":{"width":400,"height":400}},
        {"op":"create_node","id":"tail","kind":"frame","parentId":"p1","props":{"width":100,"height":100}},
        {"op":"create_node","id":"f2","kind":"frame","parentId":"p2","props":{"width":400,"height":400}},
        {"op":"create_node","id":"r1","kind":"rectangle","parentId":"f1","props":{"x":0.0,"width":20,"height":20,"fill":{"styleId":"fill"},"links":[{"to":"f2"}]}},
        {"op":"create_node","id":"r2","kind":"rectangle","parentId":"f2","props":{"width":20,"height":20}},
        {"op":"create_node","id":"leaf","kind":"frame","parentId":"library","props":{"component":{"name":"Leaf"},"width":40,"height":40}},
        {"op":"create_node","id":"label","kind":"text","parentId":"leaf","props":{"text":"Hi","width":20,"height":20,"fontSize":12,"lineHeight":16,"styleId":"text-style"}},
        {"op":"create_node","id":"master","kind":"frame","parentId":"library","props":{"component":{"name":"Master"},"width":100,"height":100}},
        {"op":"create_node","id":"nested","kind":"instance","parentId":"master","props":{"componentId":"leaf"}},
        {"op":"create_node","id":"label2","kind":"text","parentId":"master","props":{"text":"Direct","width":20,"height":20,"fontSize":12,"lineHeight":16,"styleId":"text-style","fill":{"styleId":"fill"}}},
        {"op":"create_node","id":"instance","kind":"instance","parentId":"f1","props":{"componentId":"master","overrides":{"label2":{"fill":"#ff0000"}}}},
        {"op":"create_note","id":"note","nodeId":"r2","text":"Anchored","author":"tester"}
    ])).unwrap() }).unwrap();
    p.snapshot().unwrap()
}

#[test]
fn i5_persistent_dependency_adapter_matches_legacy() {
    let before = seed();
    let view = fagent_scene_input::read_scope::ReadView::new(before.clone());
    for pages in [
        vec![],
        vec!["missing".into()],
        vec!["p1".into(), "p1".into()],
        vec!["p1".into()],
        vec!["p2".into()],
        vec!["library".into()],
        vec!["empty".into()],
        vec!["p1".into(), "p2".into()],
    ] {
        let public =
            |result: Result<BTreeSet<String>>| result.map_err(|e| serde_json::to_value(e).unwrap());
        assert_eq!(
            public(view.dependency_closure(&pages)),
            public(super::dependencies::closure(&before, &pages)),
            "{pages:?}"
        );
    }
}

pub(crate) fn snapshot(before: &Snapshot, candidate: &Candidate) -> Snapshot {
    let mut after = before.clone();
    after.nodes = candidate.nodes.values().cloned().collect();
    after.tokens = candidate.tokens.values().cloned().collect();
    after.styles = candidate.styles.values().cloned().collect();
    after.fonts = candidate.fonts.values().cloned().collect();
    after.assets = candidate.assets.values().cloned().collect();
    after.notes = candidate.notes.values().cloned().collect();
    after
}

fn closures(snapshot: &Snapshot, closure: impl Fn(&[String]) -> Result<BTreeSet<String>>) {
    for pages in [
        vec![],
        vec!["missing".into()],
        vec!["p1".into(), "p1".into()],
        vec!["p1".into()],
        vec!["p2".into()],
        vec!["library".into()],
        vec!["p1".into(), "p2".into()],
    ] {
        let result = closure(&pages).map_err(|e| serde_json::to_value(e).unwrap());
        let reference = super::dependencies::closure(snapshot, &pages)
            .map_err(|e| serde_json::to_value(e).unwrap());
        assert_eq!(result, reference, "pages={pages:?}");
    }
}

/// Index closures of `snapshot` match the reference.
fn snapshot_closures(snapshot: &Snapshot) {
    let index = DependencyIndex::from_snapshot(snapshot).unwrap();
    closures(snapshot, |pages| index.closure(pages));
}

/// Stages `operations` in `NodeMemory` as a commit does and checks its footprint,
/// the full-scan footprint and the closures of the result against the reference.
fn step(before: &Snapshot, operations: Vec<Operation>) -> (Snapshot, BTreeSet<String>) {
    let mut candidate = Candidate::from_snapshot(before);
    let mut changed = BTreeSet::new();
    for op in &operations {
        candidate.apply(op, &mut changed).unwrap();
    }
    candidate.prune_links(&mut changed);
    let expected = reference_history::footprint(before, &candidate).unwrap();
    assert_eq!(
        history::footprint(before, &candidate).unwrap(),
        expected,
        "full scan, operations={operations:?}"
    );
    let engine = NodeMemory::new(MemoryConfig::default(), Arc::new(SystemClock::new()));
    let project = engine.load(before.clone()).unwrap();
    let Begin::Ready(prepared) = project.begin(before.revision, &operations).unwrap() else {
        panic!("stale base");
    };
    assert_eq!(
        prepared.footprint(),
        &expected,
        "engine, operations={operations:?}"
    );
    let after = snapshot(before, &candidate);
    closures(&after, |pages| prepared.after().dependency_closure(pages));
    snapshot_closures(&after);
    (after, expected)
}

#[test]
fn closures_and_footprints_match_frozen_reference() {
    let before = seed();
    snapshot_closures(&before);
    let cases = [
        json!([{"op":"update_node","id":"r1","props":{"name":"Changed"}}]),
        json!([{"op":"update_node","id":"r1","props":{"x":-0.0}}]),
        json!([{"op":"update_node","id":"r1","props":{"x":0.0}}]),
        json!([{"op":"update_node","id":"r1","props":{"name":"Temporary"}},{"op":"update_node","id":"r1","props":{"name":"Temporary"}}]),
        json!([{"op":"reparent_node","id":"r1","parentId":"f2"}]),
        json!([{"op":"delete_subtree","id":"f2"}]),
        json!([{"op":"reorder_node","id":"tail","index":0}]),
        json!([{"op":"delete_subtree","id":"tail"}]),
        json!([{"op":"create_node","id":"append","kind":"frame","parentId":"p1","props":{"width":10,"height":10}}]),
        json!([{"op":"create_node","id":"first","kind":"frame","parentId":"empty","props":{"width":10,"height":10}}]),
        json!([{"op":"update_node","id":"f2","props":{"flowStart":"Start"}}]),
        json!([{"op":"upsert_token","token":{"id":"base","name":"Base","type":"color","value":"#aabbcc"}}]),
        json!([{"op":"update_node","id":"instance","props":{"theme":"night"}}]),
        json!([{"op":"update_node","id":"label","props":{"text":"Master changed"}}]),
        json!([{"op":"update_note","id":"note","text":"Edited"}]),
        json!([{"op":"delete_subtree","id":"p2"}]),
        json!([{"op":"reorder_node","id":"p2","index":0}]),
    ];
    for case in cases {
        step(&before, serde_json::from_value(case).unwrap());
    }
    let (_, keys) = step(
        &before,
        serde_json::from_value(json!([{"op":"delete_subtree","id":"f2"}])).unwrap(),
    );
    for expected in ["node:r1", "node:r2", "note:note", "page:p1", "page:p2", "*"] {
        assert!(keys.contains(expected), "{keys:?}");
    }
    let (_, keys) = step(
        &before,
        serde_json::from_value(json!([{"op":"reparent_node","id":"r1","parentId":"f2"}])).unwrap(),
    );
    assert!(keys.contains("page:p1") && keys.contains("page:p2"));
}

#[test]
fn resource_import_full_fallback_preserves_old_and_new_font_families_and_assets() {
    let mut before = seed();
    before
        .fonts
        .iter_mut()
        .find(|f| f.id == "font_bundled")
        .unwrap()
        .asset_id = Some("font-before".into());
    let mut candidate = Candidate::from_snapshot(&before);
    let font = candidate.fonts.get_mut("font_bundled").unwrap();
    font.family = "Replacement".into();
    font.asset_id = Some("font-after".into());
    candidate.assets.insert("font-after".into(), serde_json::from_value(json!({"id":"font-after","contentHash":"bytes-changed","mediaType":"font/ttf","byteSize":1,"relativeKey":"font-key","status":"ready"})).unwrap());
    let new = history::footprint(&before, &candidate).unwrap();
    assert_eq!(
        new,
        reference_history::footprint(&before, &candidate).unwrap()
    );
    assert!(new.contains("font:font_bundled"));
    assert!(new.contains("font-family:Replacement"));
    assert!(new.contains("asset:font-after"));
    assert!(new.contains(&format!(
        "font-family:{}",
        before
            .fonts
            .iter()
            .find(|f| f.id == "font_bundled")
            .unwrap()
            .family
    )));
    let after = snapshot(&before, &candidate);
    snapshot_closures(&after);
    let index = DependencyIndex::from_snapshot(&after).unwrap();
    assert!(index
        .closure(&["p1".into()])
        .unwrap()
        .contains("asset:font-after"));
}

#[test]
fn instance_free_project_closures_and_footprints_match_frozen_reference() {
    let mut before = seed();
    before.nodes.retain(|n| {
        !matches!(
            n.id.as_str(),
            "instance" | "nested" | "label" | "label2" | "master" | "leaf"
        )
    });
    Candidate::from_snapshot(&before).validate().unwrap();
    step(
        &before,
        serde_json::from_value(
            json!([{"op":"update_node","id":"r1","props":{"name":"No instances"}}]),
        )
        .unwrap(),
    );
    step(
        &before,
        serde_json::from_value(json!([{"op":"update_node","id":"f1","props":{"theme":"night"}}]))
            .unwrap(),
    );
}
