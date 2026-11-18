//! Ids a discarded batch interned leave the project's shared interner again, unless a
//! view of the discarded version is still alive or another batch staged meanwhile; the
//! handle's byte estimate then counts what stays.
mod support;

use fagent_node_memory::{
    Begin, HotProject, ManualClock, MemoryConfig, NodeMemory, Prepared, Registry, RegistryConfig,
};
use fagent_scene_input::{NodeKind, Operation};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use support::*;

/// A create that is rejected after staging named a 10 KB master and link target.
fn rejected(i: usize) -> Vec<Operation> {
    let long = |c: char| {
        format!(
            "{c}{i}-{}",
            std::iter::repeat_n(c, 10_000).collect::<String>()
        )
    };
    let props = json!({"componentId": long('m'), "links": [{"to": long('t')}]});
    vec![create(&format!("x{i}"), NodeKind::Instance, "f1", props)]
}

/// A valid create of a node with a long id.
fn long_create() -> Vec<Operation> {
    let id: String = std::iter::repeat_n('y', 120).collect();
    vec![create(&id, NodeKind::Rectangle, "f1", json!({}))]
}

fn handle() -> Arc<fagent_node_memory::ProjectHandle> {
    let config = RegistryConfig {
        ttl: Duration::from_secs(60),
        budget_bytes: usize::MAX,
    };
    let registry = Registry::new(config, Arc::new(ManualClock::default()));
    registry.load(small()).unwrap().0
}

#[test]
fn rejected_batches_do_not_grow_the_project() {
    let memory = NodeMemory::new(MemoryConfig::default(), Arc::new(ManualClock::default()));
    let project = memory.load(small()).unwrap();
    let before = project.estimated_bytes();
    for i in 0..200 {
        assert!(project.begin(1, &rejected(i)).is_err());
    }
    let Begin::Ready(prepared) = project
        .begin(1, &[update("b", json!({"name":"B"}), true)])
        .unwrap()
    else {
        panic!("stale");
    };
    prepared.commit();
    let grown = project.estimated_bytes() - before;
    assert!(grown < 4_096, "grew by {grown} bytes");
}

#[test]
fn a_rejected_or_dropped_batch_restores_the_exact_estimate() {
    let mut state = load(small());
    let before = state.estimated_bytes();
    assert!(state.begin(&rejected(0)).is_err());
    assert_eq!(state.estimated_bytes(), before);
    drop(state.begin(&long_create()).unwrap());
    assert_eq!(state.estimated_bytes(), before);

    let handle = handle();
    let before = handle.estimated_bytes();
    assert!(handle.begin(&rejected(0)).is_err());
    handle.begin(&long_create()).unwrap().abort();
    assert_eq!(handle.estimated_bytes(), before);
    assert_eq!(handle.read().estimated_bytes(), before);
}

#[test]
fn an_escaped_preview_keeps_its_names_and_is_counted() {
    let handle = handle();
    let before = handle.estimated_bytes();
    let mut staged = handle.begin(&long_create()).unwrap();
    let preview = staged.preview();
    staged.abort();
    let id: String = std::iter::repeat_n('y', 120).collect();
    assert!(preview.node(&id).is_some());
    assert!(handle.read().node(&id).is_none());
    // The names stay interned; the estimate counts them until the next commit.
    assert!(handle.estimated_bytes() > before);
    assert_eq!(handle.estimated_bytes(), handle.read().estimated_bytes());
}

#[test]
fn a_discarded_batch_keeps_names_another_batch_interned_meanwhile() {
    let base = load(small());
    let (mut first, mut second) = (base.clone(), base.clone());
    let staged = first.begin(&long_create()).unwrap();
    second
        .apply(&[create("z", NodeKind::Rectangle, "f1", json!({}))])
        .unwrap();
    assert!(second.begin(&rejected(0)).is_err());
    drop(staged);
    assert!(second.node("z").is_some());
    second
        .apply(&[create("w", NodeKind::Rectangle, "f1", json!({}))])
        .unwrap();
    assert!(second.node("w").is_some() && second.node("z").is_some());
    assert_rebuild_matches(&second);
    assert_rebuild_matches(&first);
}

fn prepared(project: &HotProject) -> Prepared<'_> {
    match project.begin(1, &long_create()).unwrap() {
        Begin::Ready(prepared) => prepared,
        Begin::Stale => panic!("stale"),
    }
}

#[test]
fn a_rolled_back_prepared_batch_discards_its_names_unless_its_view_escaped() {
    let memory = NodeMemory::new(MemoryConfig::default(), Arc::new(ManualClock::default()));
    let project = memory.load(small()).unwrap();
    let before = project.estimated_bytes();
    drop(prepared(&project));
    assert_eq!(project.estimated_bytes(), before);
    let after = prepared(&project).after().clone();
    let id: String = std::iter::repeat_n('y', 120).collect();
    assert!(after.node(&id).is_some());
    assert!(project.estimated_bytes() > before);
    assert_eq!(memory.stats().rollbacks, 2);
}

#[test]
fn names_a_rejected_batch_keeps_are_counted_by_the_handle() {
    let handle = handle();
    let before = handle.estimated_bytes();
    // Another batch on a clone of the same state is open: the rejected batch's names
    // cannot be told apart from it, so they stay.
    let mut other = handle.read().clone();
    let staged = other.begin(&long_create()).unwrap();
    let opened = handle.read().estimated_bytes();
    assert!(handle.begin(&rejected(0)).is_err());
    let kept = handle.read().estimated_bytes() - opened;
    assert!(kept > 20_000, "kept {kept} bytes");
    assert_eq!(handle.estimated_bytes() - before, kept);
    drop(staged);
}
