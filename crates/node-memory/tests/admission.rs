//! Admission at `NodeMemory::load`: invalid stored data is loaded, served and counted,
//! and later batches are judged on their delta only (pure diff-only validation): a
//! stored defect matters only when the batch's closure reaches it. An unrelated batch
//! is accepted; a batch that touches the defect is rejected for it.
mod support;

use fagent_node_memory::{Begin, HotProject, ManualClock, MemoryConfig, NodeMemory};
use fagent_scene_input::{NodeKind, Operation, Snapshot};
use serde_json::{json, Value};
use std::sync::Arc;
use support::*;

fn corrupt(id: &str, props: Value) -> Snapshot {
    let mut snapshot = small();
    snapshot
        .nodes
        .iter_mut()
        .find(|n| n.id == id)
        .unwrap()
        .props = props;
    snapshot
}

fn with_note(mut snapshot: Snapshot, node: &str, text: &str) -> Snapshot {
    let note = json!({"id":"n1","nodeId":node,"text":text});
    snapshot.notes.push(serde_json::from_value(note).unwrap());
    snapshot
}

fn setup(snapshot: Snapshot) -> (NodeMemory, HotProject) {
    let memory = NodeMemory::new(MemoryConfig::default(), Arc::new(ManualClock::default()));
    let project = memory
        .load(snapshot)
        .expect("invalid stored data still loads");
    (memory, project)
}

fn admitted(memory: &NodeMemory, failures: u64) {
    let stats = memory.stats();
    assert_eq!((stats.loads, stats.admissions), (1, 1));
    assert_eq!(stats.admission_failures, failures);
}

/// Begins and commits `ops` on the published revision; panics with the error when
/// rejected.
fn accept(project: &HotProject, ops: &[Operation]) {
    let revision = project.view().metadata().revision;
    match project.begin(revision, ops) {
        Ok(Begin::Ready(prepared)) => {
            prepared.commit();
        }
        Ok(Begin::Stale) => panic!("stale"),
        Err(error) => panic!("rejected {ops:?}: {error:?}"),
    }
}

/// Begins `ops` on the published revision and returns the rejection's field.
fn reject(project: &HotProject, ops: &[Operation]) -> Option<String> {
    let revision = project.view().metadata().revision;
    match project.begin(revision, ops) {
        Ok(Begin::Ready(_)) => panic!("accepted {ops:?}"),
        Ok(Begin::Stale) => panic!("stale"),
        Err(error) => error.field,
    }
}

/// Batches that touch none of the corrupted rows.
fn unrelated_batches() -> Vec<Vec<Operation>> {
    vec![
        vec![update("b", json!({"name":"B"}), true)],
        vec![create("x", NodeKind::Rectangle, "f1", json!({}))],
        vec![reorder("b", 1)],
        vec![reparent("b", "g")],
        vec![delete("b")],
    ]
}

/// One corrupted base: every unrelated batch is accepted (each on a fresh load), and
/// `touching` is rejected for `field`.
fn judged_on_delta(case: &str, snapshot: &Snapshot, touching: &[Operation], field: &str) {
    for ops in unrelated_batches() {
        let (memory, project) = setup(snapshot.clone());
        admitted(&memory, 1);
        accept(&project, &ops);
    }
    let (_, project) = setup(snapshot.clone());
    assert_eq!(reject(&project, touching).as_deref(), Some(field), "{case}");
}

#[test]
fn a_valid_project_is_admitted() {
    let (memory, project) = setup(small());
    admitted(&memory, 0);
    accept(&project, &[update("b", json!({"name":"B"}), true)]);
}

#[test]
fn an_invalid_property() {
    let snapshot = corrupt("a", json!({"width":false}));
    let (memory, project) = setup(snapshot.clone());
    admitted(&memory, 1);
    assert_eq!(project.view().node("a").unwrap().props["width"], false);
    judged_on_delta(
        "an invalid property",
        &snapshot,
        &[update("a", json!({"name":"A"}), true)],
        "width",
    );
    // A batch that repairs the row is accepted.
    accept(&project, &[update("a", json!({"width":10}), true)]);
}

#[test]
fn a_dangling_alias() {
    let mut snapshot = small();
    let token = json!({"id":"t1","name":"T1","type":"color","value":{"aliasId":"gone"}});
    snapshot.tokens = vec![serde_json::from_value(token).unwrap()];
    // A node starting to use the token resolves the dangling alias.
    let touching = [update("b", json!({"fill":{"tokenId":"t1"}}), true)];
    judged_on_delta("a dangling alias", &snapshot, &touching, "token.aliasId");
    let (_, project) = setup(snapshot);
    let repair: Operation = serde_json::from_value(json!({"op":"upsert_token",
        "token":{"id":"t1","name":"T1","type":"color","value":"#ffffff"}}))
    .unwrap();
    accept(&project, &[repair]);
    accept(&project, &touching);
}

#[test]
fn a_broken_component_id() {
    let snapshot = corrupt("i2", json!({"componentId":"gone"}));
    let touching = [update("i2", json!({"name":"I"}), true)];
    judged_on_delta(
        "a broken componentId",
        &snapshot,
        &touching,
        "props.componentId",
    );
    let (_, project) = setup(snapshot);
    accept(&project, &[update("i2", json!({"componentId":"m2"}), true)]);
}

#[test]
fn an_empty_note() {
    let snapshot = with_note(small(), "t2", "");
    let touching = [Operation::UpdateNote {
        id: "n1".into(),
        text: None,
        resolved: Some(true),
        node_id: None,
    }];
    judged_on_delta("an empty note", &snapshot, &touching, "text");
}

#[test]
fn a_link_to_a_non_root_frame() {
    let mut link = text("T");
    link["links"] = json!([{"to":"a"}]);
    let snapshot = corrupt("c", link);
    let touching = [update("c", json!({"name":"C"}), true)];
    judged_on_delta(
        "a link to a non-root frame",
        &snapshot,
        &touching,
        "links[0].to",
    );
}

#[test]
fn a_trigger_that_is_not_an_ancestor() {
    let animation = json!({"name":"fade","on":"hover","triggerNodeId":"b","duration":200,
        "keyframes":[{"opacity":0.5}]});
    let snapshot = corrupt("a", json!({"animations":[animation]}));
    let touching = [update("a", json!({"name":"A"}), true)];
    let field = "props.animations[0].triggerNodeId";
    judged_on_delta(
        "a trigger that is not an ancestor",
        &snapshot,
        &touching,
        field,
    );
}

#[test]
fn a_duplicate_flow_start() {
    let frame = |flow: &str| json!({"width":100,"height":100,"flowStart":flow});
    let mut snapshot = corrupt("f1", frame("x"));
    snapshot
        .nodes
        .iter_mut()
        .find(|n| n.id == "f2")
        .unwrap()
        .props = frame("x");
    let touching = [update("f1", json!({"name":"F"}), true)];
    judged_on_delta("a duplicate flow start", &snapshot, &touching, "flowStart");
}

/// A delete that cascades to a note is judged on its delta like any other batch: the
/// stored error elsewhere does not reject it.
#[test]
fn a_note_cascade_on_an_invalid_base_is_judged_on_its_delta() {
    let snapshot = with_note(corrupt("t2", json!({"width":false})), "gt", "Hello");
    let (memory, project) = setup(snapshot);
    admitted(&memory, 1);
    accept(&project, &[delete("g")]);
    assert!(project.view().note("n1").is_none());
}

fn with_notes(count: usize) -> Snapshot {
    let mut snapshot = small();
    for i in 0..count {
        let note = json!({"id": format!("n{i}"), "nodeId": "a", "text": "hi"});
        snapshot.notes.push(serde_json::from_value(note).unwrap());
    }
    snapshot
}

#[test]
fn too_many_stored_notes() {
    let snapshot = with_notes(fagent_scene_input::note_limits().notes + 1);
    let touching = [Operation::CreateNote {
        id: "extra".into(),
        node_id: "b".into(),
        text: "one more".into(),
        author: None,
    }];
    judged_on_delta("too many stored notes", &snapshot, &touching, "notes");
    // A batch that keeps the count is not reached; under the limit again, notes fit.
    let (_, project) = setup(snapshot);
    let remove = |id: &str| Operation::DeleteNote { id: id.into() };
    accept(&project, &[remove("n0"), touching[0].clone()]);
    accept(&project, &[remove("n1"), remove("n2")]);
    accept(
        &project,
        &touching.map(|op| match op {
            Operation::CreateNote {
                node_id,
                text,
                author,
                ..
            } => Operation::CreateNote {
                id: "extra2".into(),
                node_id,
                text,
                author,
            },
            op => op,
        }),
    );
}
