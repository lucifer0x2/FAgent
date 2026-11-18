//! Publication smoke tests: isolation, stable source order and delta-maintained reads.
mod support;

use fagent_node_memory::{ManualClock, Operation, Registry, RegistryConfig};
use fagent_scene_input::{
    read_scope::{ReadScope, ScopeIndex, ScopedSnapshot},
    NodeKind,
};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use support::*;

fn handle() -> Arc<fagent_node_memory::ProjectHandle> {
    Registry::new(
        RegistryConfig {
            ttl: Duration::from_secs(60),
            budget_bytes: usize::MAX,
        },
        Arc::new(ManualClock::default()),
    )
    .load(small())
    .unwrap()
    .0
}

#[test]
fn published_readers_keep_their_revision_without_blocking_a_commit() {
    let handle = handle();
    let old = handle.read_view();
    let old_scoped = old.scoped();
    let before_bytes = handle.estimated_bytes();
    let staged = handle
        .begin(&[update("m1t", json!({"text":"new"}), false)])
        .unwrap();
    // No writer-state lock is needed, even while persistence owns the staged batch.
    assert!(Arc::ptr_eq(&old, &handle.read_view()));
    assert_eq!(old_scoped.node("m1t").unwrap().props["text"], "T");
    staged.abort();
    assert!(Arc::ptr_eq(&old, &handle.read_view()));
    assert_eq!(handle.estimated_bytes(), before_bytes);

    let mut staged = handle
        .begin(&[update("m1t", json!({"text":"new"}), false)])
        .unwrap();
    let preview = staged.preview();
    assert!(Arc::ptr_eq(&old, &handle.read_view()));
    staged.commit();
    let new = handle.read_view();
    assert!(Arc::ptr_eq(&preview, &new));
    assert_eq!(new.metadata().revision, 2);
    assert_eq!(old.metadata().revision, 1);
    assert_eq!(old_scoped.node("m1t").unwrap().props["text"], "T");
    let expanded = new
        .scoped()
        .expand(ReadScope::Target("i1/m1t"), true)
        .unwrap();
    assert_eq!(
        expanded
            .expanded
            .iter()
            .find(|n| n.id == "i1/m1t")
            .unwrap()
            .props["text"],
        "new"
    );
    // One persistent state: the published view is the writer's version, so the
    // project counts once, and untouched nodes are shared with the old revision.
    assert_eq!(handle.estimated_bytes(), handle.read().estimated_bytes());
    assert!(std::ptr::eq(
        old.node("m1").unwrap(),
        new.node("m1").unwrap()
    ));
    assert!(!std::ptr::eq(
        old.node("m1t").unwrap(),
        new.node("m1t").unwrap()
    ));
}

fn check(handle: &fagent_node_memory::ProjectHandle) {
    let mut snapshot = handle.read().to_snapshot();
    snapshot
        .nodes
        .sort_by(|a, b| (&a.parent_id, a.sort_key, &a.id).cmp(&(&b.parent_id, b.sort_key, &b.id)));
    let expected = ScopedSnapshot::from_validated(&snapshot, Arc::new(ScopeIndex::new(&snapshot)));
    let view = handle.read_view();
    let actual = view.scoped();
    assert_eq!(view.len(), snapshot.nodes.len());
    for node in &snapshot.nodes {
        assert_eq!(actual.node(&node.id), Some(node));
        assert_eq!(
            view.page(&node.id),
            Some(handle.read().page_of(&node.id).unwrap().id.as_str())
        );
        let a = actual.expand(ReadScope::Target(&node.id), true).unwrap();
        let b = expected.expand(ReadScope::Target(&node.id), true).unwrap();
        assert_eq!(a.authored, b.authored, "authored {}", node.id);
        assert_eq!(a.dependencies, b.dependencies, "dependencies {}", node.id);
        assert_eq!(a.expanded, b.expanded, "expanded {}", node.id);
        assert_eq!(
            actual.navigation_nodes(&a.authored, &node.id),
            expected.navigation_nodes(&b.authored, &node.id)
        );
        assert_eq!(
            actual.notes_for_nodes([node.id.as_str()]),
            expected.notes_for_nodes([node.id.as_str()])
        );
    }
}

#[test]
fn persistent_indexes_match_fresh_scope_after_structural_and_reference_deltas() {
    let handle = handle();
    check(&handle);
    let batches = vec![
        vec![
            create("new", NodeKind::Text, "f1", text("new")),
            reorder("a", 0),
        ],
        vec![reparent("g", "f2")],
        vec![update("i1", json!({"componentId":"m2"}), false)],
        vec![update(
            "a",
            json!({"links":[{"to":"f2","trigger":"click"}]}),
            false,
        )],
        vec![update(
            "a",
            json!({"fill":{"tokenId":"paint"},"states":{"hover":{"fill":{"styleId":"s"}}}}),
            false,
        )],
        vec![Operation::CreateNote {
            id: "note".into(),
            node_id: "new".into(),
            text: "note".into(),
            author: None,
        }],
        vec![Operation::UpdateNote {
            id: "note".into(),
            node_id: Some("gt".into()),
            text: Some("moved".into()),
            resolved: None,
        }],
        vec![Operation::DeleteNote { id: "note".into() }],
        vec![delete("new"), delete("a")],
    ];
    for batch in batches {
        handle.apply(&batch).unwrap();
        check(&handle);
    }
    assert_eq!(
        handle.read_view().reference_users("component:m1").count(),
        0
    );
    assert_eq!(
        handle.read_view().reference_users("component:m2").count(),
        2
    );
    assert_eq!(handle.read_view().reference_users("token:paint").count(), 0);
}
