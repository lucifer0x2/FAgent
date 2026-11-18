//! Admission at `NodeMemory::load`: invalid stored data is loaded, served, counted
//! and certified when its instances expand; later commits are judged like the
//! reference judges them.
mod support;

use fagent_node_memory::{Begin, HotProject, ManualClock, MemoryConfig, NodeMemory, Path};
use fagent_scene_input::validate::{memory::certified, Candidate};
use fagent_scene_input::{Operation, Result, Snapshot, StorageError};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Arc;
use support::*;

/// The engine's reference (`batch::reference`): apply, prune links, `validate_after`.
fn reference(snapshot: &Snapshot, ops: &[Operation]) -> Result<Vec<String>> {
    let mut candidate = Candidate::from_snapshot(snapshot);
    let mut changed = BTreeSet::new();
    for (index, op) in ops.iter().enumerate() {
        candidate.apply(op, &mut changed).map_err(|e| e.at(index))?;
    }
    candidate.prune_links(&mut changed);
    candidate.validate_after(snapshot, ops, &changed)?;
    Ok(changed.into_iter().collect())
}

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

fn setup(snapshot: Snapshot) -> (NodeMemory, HotProject) {
    let memory = NodeMemory::new(MemoryConfig::default(), Arc::new(ManualClock::default()));
    let project = memory
        .load(snapshot)
        .expect("invalid stored data still loads");
    (memory, project)
}

fn json_of(error: &StorageError) -> Value {
    serde_json::to_value(error).unwrap()
}

/// Begins `ops` on the published revision and compares the outcome with the
/// reference; commits an accepted batch and returns its path.
fn commit_like_reference(project: &HotProject, ops: &[Operation]) -> Option<Path> {
    let base = project.view().to_snapshot();
    let expected = reference(&base, ops);
    match (project.begin(base.revision, ops), expected) {
        (Ok(Begin::Ready(prepared)), Ok(changed)) => {
            assert_eq!(prepared.changed_ids(), changed.as_slice());
            let path = prepared.path();
            prepared.commit();
            Some(path)
        }
        (Err(actual), Err(expected)) => {
            assert_eq!(json_of(&actual), json_of(&expected));
            None
        }
        (Ok(Begin::Stale), _) => panic!("stale"),
        (Ok(_), Err(expected)) => panic!("only the reference rejects: {expected:?}"),
        (Err(actual), Ok(_)) => panic!("only the engine rejects: {actual:?}"),
    }
}

fn admitted(memory: &NodeMemory, failures: u64) {
    let stats = memory.stats();
    assert_eq!((stats.loads, stats.admissions), (1, 1));
    assert_eq!(stats.admission_failures, failures);
}

#[test]
fn a_valid_project_is_admitted_and_certified() {
    let (memory, project) = setup(small());
    admitted(&memory, 0);
    assert!(certified(&project.view()));
}

#[test]
fn an_invalid_property_is_served_and_commits_stay_local() {
    let (memory, project) = setup(corrupt("a", json!({"width":false})));
    admitted(&memory, 1);
    assert_eq!(project.view().node("a").unwrap().props["width"], false);
    assert!(certified(&project.view()));
    let unrelated = [update("b", json!({"name":"B"}), true)];
    assert_eq!(
        commit_like_reference(&project, &unrelated),
        Some(Path::Local)
    );
    let touching = [update("a", json!({"name":"A"}), true)];
    assert_eq!(commit_like_reference(&project, &touching), None);
    assert_eq!(memory.stats().divergences, 0);
}

#[test]
fn a_dangling_alias_rejects_commits_until_the_token_is_repaired() {
    let mut snapshot = small();
    let token = json!({"id":"t1","name":"T1","type":"color","value":{"aliasId":"gone"}});
    snapshot.tokens = vec![serde_json::from_value(token).unwrap()];
    let (memory, project) = setup(snapshot);
    admitted(&memory, 1);
    assert!(certified(&project.view()));
    assert_eq!(project.view().metadata().tokens.len(), 1);
    // Every validation checks the whole token set, the reference's included.
    let unrelated = [update("b", json!({"name":"B"}), true)];
    assert_eq!(commit_like_reference(&project, &unrelated), None);
    let repair: Operation = serde_json::from_value(json!({"op":"upsert_token",
        "token":{"id":"t1","name":"T1","type":"color","value":"#ffffff"}}))
    .unwrap();
    assert_eq!(commit_like_reference(&project, &[repair]), Some(Path::Full));
    assert_eq!(
        commit_like_reference(&project, &unrelated),
        Some(Path::Local)
    );
}

#[test]
fn a_broken_component_id_leaves_no_certificate() {
    let (memory, project) = setup(corrupt("i2", json!({"componentId":"gone"})));
    admitted(&memory, 1);
    assert_eq!(
        project.view().node("i2").unwrap().props["componentId"],
        "gone"
    );
    // The instance does not expand: no certificate, commits take the full path, where
    // the reference expands every instance and rejects even an unrelated batch.
    assert!(!certified(&project.view()));
    let unrelated = [update("b", json!({"name":"B"}), true)];
    assert_eq!(commit_like_reference(&project, &unrelated), None);
    let touching = [update("i2", json!({"name":"I"}), true)];
    assert_eq!(commit_like_reference(&project, &touching), None);
    let repair = [update("i2", json!({"componentId":"m2"}), true)];
    assert_eq!(commit_like_reference(&project, &repair), Some(Path::Full));
    // The repaired revision is valid: the next batch certifies it and stays local.
    assert_eq!(
        commit_like_reference(&project, &unrelated),
        Some(Path::Local)
    );
    assert_eq!(memory.stats().divergences, 0);
}

fn with_note(mut snapshot: Snapshot, node: &str, text: &str) -> Snapshot {
    let note = json!({"id":"n1","nodeId":node,"text":text});
    snapshot.notes.push(serde_json::from_value(note).unwrap());
    snapshot
}

/// Local-class batches that do not touch the corrupted rows.
fn unrelated_batches() -> Vec<Vec<Operation>> {
    use fagent_scene_input::NodeKind;
    vec![
        vec![update("b", json!({"name":"B"}), true)],
        vec![create("x", NodeKind::Rectangle, "f1", json!({}))],
        vec![reorder("b", 1)],
        vec![reparent("b", "g")],
        vec![delete("b")],
    ]
}

/// Begins `ops` on a fresh load of `snapshot`; the outcome must be the reference's.
fn judged_like_reference(case: &str, snapshot: &Snapshot, ops: &[Operation]) {
    let (memory, project) = setup(snapshot.clone());
    let expected = reference(&project.view().to_snapshot(), ops);
    match (project.begin(snapshot.revision, ops), expected) {
        (Ok(Begin::Ready(prepared)), Ok(changed)) => {
            assert_eq!(prepared.changed_ids(), changed.as_slice(), "{case}");
        }
        (Err(actual), Err(expected)) => {
            assert_eq!(json_of(&actual), json_of(&expected), "{case}");
        }
        (Ok(Begin::Ready(_)), Err(expected)) => {
            panic!("{case}: only the reference rejects {ops:?}: {expected:?}")
        }
        (Err(actual), Ok(_)) => panic!("{case}: only the engine rejects {ops:?}: {actual:?}"),
        (Ok(_), _) => panic!("{case}: stale"),
    }
    assert_eq!(memory.stats().divergences, 0, "{case}");
}

#[test]
fn a_base_failing_a_project_wide_check_is_judged_like_the_reference() {
    let frame = |extra: Value| {
        let mut props = json!({"width":100,"height":100});
        props
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        props
    };
    let mut flows = corrupt("f1", frame(json!({"flowStart":"x"})));
    flows.nodes.iter_mut().find(|n| n.id == "f2").unwrap().props = frame(json!({"flowStart":"x"}));
    let mut link = text("T");
    link["links"] = json!([{"to":"a"}]);
    let cases = [
        ("an empty note", with_note(small(), "t2", "")),
        ("a link to a non-root frame", corrupt("c", link)),
        (
            "a trigger that is not an ancestor",
            corrupt("a", json!({"animations":[{"triggerNodeId":"b"}]})),
        ),
        ("a duplicate flow start", flows),
    ];
    for (case, snapshot) in &cases {
        let (_, project) = setup(snapshot.clone());
        assert!(!certified(&project.view()), "{case}");
        for ops in unrelated_batches() {
            judged_like_reference(case, snapshot, &ops);
        }
    }
}

#[test]
fn a_note_cascade_on_an_invalid_base_takes_the_reference_path() {
    // The reference re-validates the whole project when a delete removes a note, and
    // so reports the stored error again; the delta path must not accept that batch.
    let snapshot = with_note(corrupt("t2", json!({"width":false})), "gt", "Hello");
    let (_, project) = setup(snapshot.clone());
    assert!(certified(&project.view()));
    judged_like_reference("a note cascade", &snapshot, &[delete("g")]);
    for ops in unrelated_batches() {
        judged_like_reference("an invalid width", &snapshot, &ops);
    }
}
