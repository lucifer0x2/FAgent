//! Indexed page-frame selection matches the full source scan, errors included.
use super::ScenesQuery;
use fagent_storage::BatchRequest;
use serde_json::json;

use crate::reads::scoped_tree_fixture as fixture;

#[test]
fn shared_frames_match_full_frames_order_offsets_and_errors() {
    let (_root, storage, id) = fixture::fixture();
    let project = storage.open_project(&id).unwrap();
    let mut ops: Vec<_> = (0..3)
        .map(|i| {
            fixture::node(
                &format!("extra{i}"),
                "frame",
                Some("page"),
                json!({"width":10,"height":10}),
            )
        })
        .collect();
    ops.push(json!({"op":"reorder_node","id":"extra2","index":0}));
    ops.push(fixture::node(
        "nested-frame",
        "frame",
        Some("frame"),
        json!({"width":5,"height":5}),
    ));
    let request =
        json!({"projectId":id,"expectedRevision":1,"idempotencyKey":"frames","operations":ops});
    project
        .apply_batch(serde_json::from_value::<BatchRequest>(request).unwrap())
        .unwrap();
    let shared = storage.shared_snapshot(&id).unwrap();
    assert!(shared.trusted_scoped().unwrap().is_some());
    let mut compared = 0;
    for page in ["page", "library", "frame", "missing"] {
        for revision in [1, 2] {
            for offset in [0, 2, 4, 5] {
                let query: ScenesQuery = serde_json::from_value(
                    json!({"pageId":page,"revision":revision,"offset":offset}),
                )
                .unwrap();
                let ids = |frames: Vec<&fagent_storage::Node>| {
                    frames.into_iter().map(|n| n.id.clone()).collect::<Vec<_>>()
                };
                let full = query
                    .frames(&shared.snapshot)
                    .map(ids)
                    .map_err(|e| e.value());
                let indexed = query.shared_frames(&shared).map(ids).map_err(|e| e.value());
                assert_eq!(indexed, full, "{page} r{revision} +{offset}");
                compared += usize::from(full.as_ref().is_ok_and(|f| f.len() > 1));
            }
        }
    }
    assert!(compared >= 3);
}

#[test]
fn i5_hot_scene_fonts_and_prerender_consumers_match_full() {
    use fagent_storage::{Storage, Store};
    let (root, storage, id) = fixture::fixture();
    let full = storage.shared_snapshot(&id).unwrap();
    drop(storage);
    let hot: Storage = Store::open(root.path()).unwrap().into();
    hot.reader_snapshot(&id).unwrap();
    let memory = hot.reader_snapshot(&id).unwrap();
    assert!(memory.from_memory && memory.shared.snapshot.nodes.is_empty());
    let ids = |frames: Vec<&fagent_storage::Node>| {
        frames.into_iter().map(|n| n.id.clone()).collect::<Vec<_>>()
    };
    for page in ["page", "library", "frame", "missing"] {
        for revision in [0, 1] {
            for offset in [0, 1, 4] {
                let query: ScenesQuery = serde_json::from_value(
                    json!({"pageId":page,"revision":revision,"offset":offset}),
                )
                .unwrap();
                assert_eq!(
                    query
                        .shared_frames(&memory.shared)
                        .map(ids)
                        .map_err(|e| e.value()),
                    query.frames(&full.snapshot).map(ids).map_err(|e| e.value())
                );
            }
        }
    }
    for changed in [vec!["copy"], vec!["outgoing"], vec!["missing"], vec![]] {
        assert_eq!(
            ids(crate::reads::targets::prerender_shared(&memory.shared, &changed, 4).unwrap()),
            ids(crate::reads::targets::prerender_shared(&full, &changed, 4).unwrap())
        );
    }
    assert!(!full.snapshot.fonts.is_empty());
    for font in &full.snapshot.fonts {
        assert_eq!(
            crate::fonts::metadata(&memory.shared.snapshot, &font.id, "http://example.test")
                .unwrap(),
            crate::fonts::metadata(&full.snapshot, &font.id, "http://example.test").unwrap()
        );
    }
    assert_eq!(
        serde_json::to_value(hot.snapshot(&id).unwrap()).unwrap(),
        serde_json::to_value(full.snapshot.as_ref()).unwrap()
    );
    assert_eq!(
        serde_json::to_value(hot.shared_snapshot(&id).unwrap().snapshot.as_ref()).unwrap(),
        serde_json::to_value(full.snapshot.as_ref()).unwrap()
    );
}
