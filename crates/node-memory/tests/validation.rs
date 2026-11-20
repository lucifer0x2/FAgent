//! Every structural failure, each run after a prefix of successful operations so the
//! batch must roll back: rows, resources, every index, stamps and the byte estimate
//! exactly as before.
mod support;

use fagent_node_memory::{ManualClock, MemoryConfig, NodeMemory, Operation};
use fagent_scene_input::{NodeKind, Token};
use serde_json::{json, Value};
use std::sync::Arc;
use support::*;

/// A prefix touching every kind of change: create, update, reparent (across pages and
/// into a master), reorder, delete with a note cascade and link pruning, notes, tokens.
fn prefix() -> Vec<Operation> {
    let master = json!({"width":10,"height":10,"component":{"name":"N"}});
    vec![
        create("new", NodeKind::Frame, "f2", master),
        create(
            "newi",
            NodeKind::Instance,
            "f2",
            json!({"componentId":"new"}),
        ),
        update("i1", json!({"componentId":"m2"}), false),
        reparent("g", "m1"),
        reorder("b", 2),
        Operation::CreateNote {
            id: "note".into(),
            node_id: "c".into(),
            text: "hi".into(),
            author: None,
        },
        Operation::CreateNote {
            id: "keep".into(),
            node_id: "f1".into(),
            text: "kept".into(),
            author: None,
        },
        update(
            "a",
            json!({"links":[{"to":"t2"},{"to":"f2","on":"hover","action":"overlay"}]}),
            false,
        ),
        delete("c"),
        delete("t2"),
        update("m1n", json!({"component":null}), true),
        Operation::UpsertToken {
            token: Token {
                id: "tok".into(),
                name: "Tok".into(),
                token_type: "color".into(),
                value: json!("#000000"),
            },
        },
    ]
}

fn assert_fails_at(last: Operation, code: &str, field: Option<&str>, at_last: bool) {
    let mut state = load(small());
    let before = observe(&state);
    let mut ops = prefix();
    let index = ops.len();
    ops.push(last);
    let error = state.apply(&ops).expect_err("batch must fail");
    assert_eq!(error.code, code, "{}", error.message);
    assert_eq!(error.field.as_deref(), field, "{}", error.message);
    assert_eq!(
        error.op_index,
        at_last.then_some(index),
        "{}",
        error.message
    );
    assert_unchanged(&state, &before);
    // The prefix alone is valid, so the rollback is what restored the state.
    let delta = state.apply(&prefix()).expect("prefix alone succeeds");
    assert_eq!(delta.notes.upserted, ["keep"], "note created and cascaded");
    assert!(delta.notes.deleted.is_empty());
    assert!(delta.changed_ids().contains(&"note".to_owned()));
    assert_eq!(
        state.node("a").unwrap().props,
        json!({"links":[{"to":"f2","on":"hover","action":"overlay"}]})
    );
}

fn assert_fails(last: Operation, code: &str, field: Option<&str>) {
    assert_fails_at(last, code, field, true);
}

const VF: &str = "VALIDATION_FAILED";
const READONLY: &str = "INSTANCE_CHILD_READONLY";

#[test]
fn create_failures_roll_back() {
    let rect = || NodeKind::Rectangle;
    let make =
        |id: &str, kind: NodeKind, parent: Option<&str>, props: Value| new(id, kind, parent, props);
    assert_fails(make("a", rect(), Some("f1"), json!({})), VF, Some("id"));
    assert_fails(make("new", rect(), Some("f1"), json!({})), VF, Some("id"));
    assert_fails(make("keep", rect(), Some("f1"), json!({})), VF, Some("id"));
    assert_fails(
        make("bad id", rect(), Some("f1"), json!({})),
        VF,
        Some("id"),
    );
    assert_fails(
        make("i1/m1t", rect(), Some("f1"), json!({})),
        READONLY,
        None,
    );
    assert_fails(make("x", rect(), Some("i1/m1t"), json!({})), READONLY, None);
    assert_fails(
        make("x", rect(), Some("nope"), json!({})),
        VF,
        Some("parentId"),
    );
    assert_fails(
        make("x", rect(), Some("gt"), json!({})),
        VF,
        Some("parentId"),
    );
    assert_fails(
        make("x", rect(), Some("x"), json!({})),
        VF,
        Some("parentId"),
    );
    assert_fails(
        make("x", NodeKind::Group, Some("x"), json!({})),
        VF,
        Some("parentId"),
    );
    assert_fails(make("x", rect(), None, json!({})), VF, Some("parentId"));
    assert_fails(
        make("x", NodeKind::Page, Some("p1"), json!({})),
        VF,
        Some("parentId"),
    );
    assert_fails(make("x", rect(), Some("f1"), json!([])), VF, Some("props"));
    assert_fails(create_at("x", rect(), "f1", 4), VF, Some("index"));
    let instance = |target: Value| make("x", NodeKind::Instance, Some("f1"), target);
    let field = Some("props.componentId");
    assert_fails(instance(json!({"componentId":"f1"})), VF, field);
    assert_fails(instance(json!({"componentId":"nope"})), VF, field);
    assert_fails(instance(json!({})), VF, field);
    // m1n stopped being a master earlier in the same batch.
    assert_fails(instance(json!({"componentId":"m1n"})), VF, field);
}

#[test]
fn update_failures_roll_back() {
    assert_fails(update("nope", json!({}), true), VF, Some("id"));
    assert_fails(update("i1/m1t", json!({}), true), READONLY, None);
    assert_fails(update("a", json!(3), true), VF, Some("props"));
    let field = Some("props.componentId");
    assert_fails(update("i2", json!({"componentId":"f1"}), true), VF, field);
    assert_fails(update("i2", json!({"componentId":null}), true), VF, field);
}

/// newi (created in the prefix) uses master "new". Unmarking "new" is no operation
/// error, as in `Candidate::apply`; the end-of-batch instance rule rejects the batch on
/// the instance, without an operation index, on the state and through the engine.
#[test]
fn unmarking_a_used_master_is_an_end_of_batch_error() {
    let mut ops = prefix();
    ops.push(update("new", json!({"component":null}), true));
    let mut state = load(small());
    let error = state.begin(&ops).err().expect("end-of-batch rules reject");
    assert_eq!(error.field.as_deref(), Some("props.componentId"));
    assert_eq!(error.op_index, None);
    let memory = NodeMemory::new(MemoryConfig::default(), Arc::new(ManualClock::default()));
    let project = memory.load(small()).unwrap();
    let error = project.begin(1, &ops).err().expect("rejected");
    assert_eq!(error.field.as_deref(), Some("props.componentId"));
    assert_eq!(error.op_index, None);
    assert_eq!(memory.stats().rejected, 1);
}

#[test]
fn tree_failures_roll_back() {
    assert_fails(reparent("p1", "f1"), VF, Some("parentId"));
    assert_fails(reparent("f1", "f1"), VF, Some("parentId"));
    assert_fails(reparent("m1", "gt"), VF, Some("parentId"));
    assert_fails(reparent("m1", "g"), VF, Some("parentId"));
    assert_fails(reparent("a", "nope"), VF, Some("parentId"));
    assert_fails(reparent("nope", "f1"), VF, Some("id"));
    assert_fails(reparent("a", "i1/m1t"), READONLY, None);
    assert_fails(reparent_at("a", "f2", Some(9)), VF, Some("index"));
    assert_fails(reorder("nope", 0), VF, Some("id"));
    assert_fails(reorder("a", 2), VF, Some("index"));
}

#[test]
fn delete_and_note_failures_roll_back() {
    assert_fails(delete("nope"), VF, Some("id"));
    assert_fails(delete("c"), VF, Some("id"));
    assert_fails(delete("i1/m1t"), READONLY, None);
    // m2 is used by i2 and (after the prefix) by i1; lib holds m1, m2 and "g".
    assert_fails(delete("m2"), "COMPONENT_IN_USE", None);
    assert_fails(delete("new"), "COMPONENT_IN_USE", None);
    let note = |id: &str, node: &str| Operation::CreateNote {
        id: id.into(),
        node_id: node.into(),
        text: "x".into(),
        author: None,
    };
    assert_fails(note("a", "f1"), VF, Some("id"));
    assert_fails(note("n2", "c"), VF, Some("nodeId"));
    assert_fails(Operation::DeleteNote { id: "note".into() }, VF, Some("id"));
}

#[test]
fn component_in_use_names_users() {
    let mut state = load(small());
    let error = state.apply(&[delete("lib")]).expect_err("users outside");
    assert_eq!(error.code, "COMPONENT_IN_USE");
    let context = error.node.expect("node context");
    assert_eq!(context.instance_ids, Some(vec!["i1".into(), "i2".into()]));
    assert_eq!(context.count, Some(2));
}

#[test]
fn empty_batch_is_rejected() {
    let mut state = load(small());
    let error = state.apply(&[]).expect_err("empty");
    assert_eq!(error.field.as_deref(), Some("operations"));
    assert_eq!(state.revision(), 1);
}
