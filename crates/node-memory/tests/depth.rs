//! Maximum depth 128 (a page is depth 1): checked on load, and per operation on the
//! created or moved node. Descendants a reparent moves deeper are found by the end-of-batch
//! rules, as by `Candidate::validate`.
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
    depth_error(state.apply(&[reparent("g", "c127")]).unwrap_err());
    assert_unchanged(&state, &before);
    state.apply(&[reparent("h", "c126")]).unwrap();
    state.apply(&[reparent("g", "c125")]).unwrap();
    assert_rebuild_matches(&state);
}

/// g lands at depth 127 + 1, but its child h would be at 129. `Candidate::apply`
/// checks only the moved node, and so does `Txn`; the end-of-batch rules check the
/// moved subtree and reject the batch, without an operation index. The working state
/// holds the deep subtree meanwhile, and walking it stays safe: moving g back in the
/// same batch is accepted.
#[test]
fn descendants_moved_too_deep_are_an_end_of_batch_error() {
    let mut state = load(snapshot(chain(127)));
    let error = state.begin(&[reparent("g", "c126")]).err().unwrap();
    assert_eq!(error.op_index, None);
    depth_error(error);
    let back = [reparent("g", "c126"), reparent("g", "c1")];
    assert_eq!(
        state.begin(&back).unwrap().state().page_of("h").unwrap().id,
        "p"
    );
    let error = state
        .apply(&[
            reparent("g", "c126"),
            create("x", NodeKind::Frame, "h", json!({})),
        ])
        .unwrap_err();
    assert_eq!(error.op_index, Some(1));
    depth_error(error);
}

/// Through `HotProject::begin`: the moved node's own depth is a per-operation error
/// returned as is; its descendants' depth is found by end-of-batch validation, with no
/// operation index.
#[test]
fn descendant_depth_is_an_end_of_batch_error() {
    let memory = NodeMemory::new(MemoryConfig::default(), Arc::new(ManualClock::default()));
    let project = memory.load(snapshot(chain(127))).unwrap();
    let error = project
        .begin(1, &[create("x", NodeKind::Frame, "c127", json!({}))])
        .err()
        .unwrap();
    assert_eq!(error.op_index, Some(0));
    depth_error(error);
    let error = project.begin(1, &[reparent("g", "c126")]).err().unwrap();
    assert_eq!(error.op_index, None, "found by the end-of-batch rules");
    depth_error(error);
    assert_eq!(memory.stats().rejected, 2);
}
