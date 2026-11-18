//! Batches where the engine once rejected with a different error than the reference
//! (`Candidate::apply` per operation + full `validate()`): the whole error, message and
//! node context included, must now be the reference's.
mod support;

use fagent_node_memory::{Operation, ProjectState};
use fagent_scene_input::{NodeKind, StorageError};
use serde_json::json;
use support::oracle::{brief, expected, same_error, Expected};
use support::scenario::base;
use support::*;

/// Applies `ops` to the state and to the reference; both must reject with the same
/// error, which is returned (per operation when `per_op`, end of batch otherwise).
fn same_rejection(state: &mut ProjectState, ops: &[Operation], per_op: bool) -> StorageError {
    let reference = match (expected(&state.to_snapshot(), ops), per_op) {
        (Expected::PerOp(e), true) | (Expected::EndOfBatch(e), false) => e,
        (Expected::PerOp(e) | Expected::EndOfBatch(e), _) => {
            panic!("reference rejects at another stage: {}", brief(&e))
        }
        (Expected::Accept(..), _) => panic!("reference accepts"),
    };
    let engine = state.apply(ops).expect_err("engine rejects");
    assert!(
        same_error(&engine, &reference),
        "errors differ\n engine:    {}\n reference: {}",
        brief(&engine),
        brief(&reference)
    );
    engine
}

/// Two chains of 100 groups, `a1`..`a100` and `b1`..`b100`, under `f1`.
fn two_chains() -> ProjectState {
    let mut prefix = Vec::new();
    for chain in ["a", "b"] {
        let mut parent = "f1".to_owned();
        for i in 1..=100 {
            let id = format!("{chain}{i}");
            prefix.push(create(&id, NodeKind::Group, &parent, json!({})));
            parent = id;
        }
    }
    base(&prefix)
}

/// After `b1` moves under `a100`, the walk up from `b100` passes `b100`..`b1` then
/// `a100`..`a1`. Moving `a{k}` under `b100` closes a cycle; the reference's walk (the
/// moved node, `b100`, then its ancestors) meets `a{k}` again within 128 entries only
/// for `k >= 73`, and fails on the depth otherwise.
#[test]
fn a_cycle_beyond_the_depth_walk_is_a_depth_error() {
    for (k, cycle) in [(1, false), (72, false), (73, true), (100, true)] {
        let mut state = two_chains();
        let ops = [reparent("b1", "a100"), reparent(&format!("a{k}"), "b100")];
        let error = same_rejection(&mut state, &ops, true);
        assert_eq!(error.op_index, Some(1));
        let message = match cycle {
            true => format!("Tree cycle detected: `b100` is `a{k}` or one of its descendants"),
            false => "Tree exceeds maximum depth 128".to_owned(),
        };
        assert_eq!(error.message, message, "a{k}");
    }
}

/// The batch closes a cycle `ms -> mp -> my -> ms` and a path of ten masters from
/// `mb`. The full check walks every master in id order and meets the cycle first.
#[test]
fn a_component_cycle_and_a_deep_path_report_the_full_walk_error() {
    let master = |id: &str| {
        create(
            id,
            NodeKind::Frame,
            "lib",
            json!({"width": 10, "height": 10, "component": {"name": id}}),
        )
    };
    let instance = |id: &str, parent: &str, target: &str| {
        create(
            id,
            NodeKind::Instance,
            parent,
            json!({"componentId": target}),
        )
    };
    let mut prefix: Vec<Operation> = ["ma", "mb", "mp", "ms", "my"]
        .into_iter()
        .map(master)
        .collect();
    prefix.extend((1..=7).map(|i| master(&format!("mx{i}"))));
    for i in 1..7 {
        let target = format!("mx{}", i + 1);
        prefix.push(instance(&format!("ix{i}"), &format!("mx{i}"), &target));
    }
    prefix.extend([
        instance("ia", "ma", "mx1"),
        instance("ip1", "mp", "mx1"),
        instance("ip2", "mp", "my"),
        instance("iy", "my", "ms"),
        instance("ib", "mb", "ms"),
    ]);
    let mut state = base(&prefix);
    let error = same_rejection(&mut state, &[instance("is", "ms", "mp")], false);
    assert_eq!(error.code, "INSTANCE_CYCLE");
}
