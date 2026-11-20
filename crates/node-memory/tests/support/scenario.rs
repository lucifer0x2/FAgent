//! Scenario helpers for the delta-rule tests: operation builders for resources, a base
//! state and `judge`, which runs a batch on the state and on the reference
//! (`Candidate::apply` per operation + full `validate()`) and checks the outcome.
use super::oracle::{brief, expected, Expected};
use super::{load, small};
use fagent_node_memory::{Operation, ProjectState};
use fagent_scene_input::{Snapshot, Style, Token};
use serde_json::Value;

pub fn token(id: &str, kind: &str, value: Value) -> Operation {
    Operation::UpsertToken {
        token: Token {
            id: id.into(),
            name: id.into(),
            token_type: kind.into(),
            value,
        },
    }
}

pub fn style(id: &str, kind: &str, value: Value) -> Operation {
    Operation::UpsertStyle {
        style: Style {
            id: id.into(),
            name: id.into(),
            kind: kind.into(),
            value,
        },
    }
}

/// `small()` plus a prefix of operations, committed (they must pass).
pub fn base(prefix: &[Operation]) -> ProjectState {
    let mut state = load(small());
    if !prefix.is_empty() {
        state.apply(prefix).expect("prefix applies");
    }
    state
}

/// The outcome a scenario expects: accepted, or an end-of-batch `(code, field)`.
pub type Want = Option<(&'static str, Option<&'static str>)>;

pub const ACCEPT: Want = None;

pub fn fails(field: &'static str) -> Want {
    Some(("VALIDATION_FAILED", Some(field)))
}

/// Runs `ops` on the state and on the reference; both must reject with the wanted
/// end-of-batch code and field (and no operation index), or both accept.
pub fn judge(state: &mut ProjectState, ops: &[Operation], want: Want) {
    let snapshot: Snapshot = state.to_snapshot();
    let reference = expected(&snapshot, ops);
    let got = state.apply(ops);
    match (got, reference, want) {
        (Ok(_), Expected::Accept(..), None) => {}
        (Err(a), Expected::EndOfBatch(e), Some((code, field))) => {
            assert_eq!(a.op_index, None, "{}", brief(&a));
            assert_eq!((a.code.as_str(), a.field.as_deref()), (code, field));
            assert_eq!((e.code.as_str(), e.field.as_deref()), (code, field));
        }
        (got, reference, _) => {
            let got = got.map(|_| ()).map_err(|e| brief(&e));
            let reference = match reference {
                Expected::Accept(..) => "accept".to_owned(),
                Expected::PerOp(e) => format!("per-op {}", brief(&e)),
                Expected::EndOfBatch(e) => format!("end {}", brief(&e)),
            };
            panic!("engine {got:?}, reference {reference}, wanted {want:?}");
        }
    }
}
