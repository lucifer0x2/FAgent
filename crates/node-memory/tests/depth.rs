//! Maximum depth 128 (a page is depth 1): checked on load, create and reparent,
//! including the descendants a reparent moves deeper, so no stored node ever exceeds it.
mod support;

use fagent_node_memory::{ManualClock, MemoryConfig, NodeMemory, ProjectState};
use fagent_scene_input::NodeKind;
use serde_json::{json, Value};
use std::sync::Arc;
use support::*;

/// Page `p` with a chain `c1`..`c{len}` of frames, plus page `q` with `g` > `h`.
fn chain(len: usize) -> Vec<Value> {
    let node = |id: String, kind: &str, parent: Option<String>| json!({"id":id,"kind":kind,"parentId":parent,"sortKey":0,"props":{}});
    let mut nodes = vec![node("p".into(), "page", None)];
    for i in 1..=len {
        let parent = if i == 1 {
            "p".into()
        } else {
            format!("c{}", i - 1)
        };
        nodes.push(node(format!("c{i}"), "frame", Some(parent)));
    }
    nodes.push(node("q".into(), "page", None));
    nodes.push(node("g".into(), "frame", Some("q".into())));
    nodes.push(node("h".into(), "frame", Some("g".into())));
    nodes
}

fn depth_error(error: fagent_scene_input::StorageError) {
    assert_eq!(error.field.as_deref(), Some("parentId"));
    assert_eq!(error.message, "Tree exceeds maximum depth 128");
}

#[test]
fn depth_limit_on_load_create_and_reparent() {
    depth_error(
        ProjectState::from_snapshot(snapshot(chain(128)))
            .err()
            .unwrap(),
    );
    let mut state = load(snapshot(chain(127)));
    let before = observe(&state);
    depth_error(
        state
            .apply(&[create("x", NodeKind::Frame, "c127", json!({}))])
            .unwrap_err(),
    );
    // g lands at depth 127 + 1, but its child h would be at 129: rejected, although
    // `Candidate::apply` checks only the moved node (full validation rejects it).
    depth_error(state.apply(&[reparent("g", "c126")]).unwrap_err());
    assert_unchanged(&state, &before);
    state.apply(&[reparent("h", "c126")]).unwrap();
    state.apply(&[reparent("g", "c125")]).unwrap();
    assert_rebuild_matches(&state);
}

/// Through `HotProject::begin`: the moved node's own depth is a per-operation error
/// returned as is; its descendants' depth is not, so that error is the reference's.
#[test]
fn descendant_depth_takes_the_reference_error() {
    let memory = NodeMemory::new(MemoryConfig::default(), Arc::new(ManualClock::default()));
    let project = memory.load(snapshot(chain(127))).unwrap();
    let error = project
        .begin(1, &[create("x", NodeKind::Frame, "c127", json!({}))])
        .err()
        .unwrap();
    assert_eq!(error.op_index, Some(0));
    depth_error(error);
    let stats = memory.stats();
    assert_eq!((stats.rejected_direct, stats.rejected_routed), (1, 0));
    let error = project.begin(1, &[reparent("g", "c126")]).err().unwrap();
    assert_eq!(error.op_index, None, "found by full validation");
    depth_error(error);
    let stats = memory.stats();
    assert_eq!((stats.rejected_direct, stats.rejected_routed), (1, 1));
    assert_eq!((stats.rejected, stats.divergences), (2, 0));
    assert!(!project.diverged());
}
