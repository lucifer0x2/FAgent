//! Two-phase batches and the RAM of last-writer-wins stamps.
use crate::memory::mib;
use crate::support::{each, row};
use crate::writes::{create_op, text_in, update_op};
use fagent_node_memory::{Operation, ProjectState};
use serde_json::json;

pub fn staged(state: &mut ProjectState, runs: usize) {
    let text = text_in(state, "root_8");
    let begin = each(
        runs,
        |_| (),
        |_| {
            let staged = state.begin(&[update_op(&text)]).unwrap();
            staged.commit()
        },
    );
    row("begin + commit (1 update)", begin, "");
    let abort = each(
        runs,
        |_| (),
        |_| {
            let staged = state.begin(&[update_op(&text)]).unwrap();
            staged.abort()
        },
    );
    row("begin + abort (1 update)", abort, "clone dropped");
    let wide = each(
        runs,
        |run| format!("bench_staged_{run}"),
        |id| {
            let staged = state
                .begin(&[create_op(id, "frame_wide", Some(0))])
                .unwrap();
            staged.abort()
        },
    );
    row(
        "begin + abort (index 0 in wide frame)",
        wide,
        "renumbered clone dropped",
    );
}

/// Writes one key on every child of the wide frame (500-op batches) and reports the
/// stamp bytes added per written node.
pub fn stamp_memory(state: &mut ProjectState) {
    let (_, before) = state.stamp_stats();
    let ids: Vec<String> = state
        .children("frame_wide")
        .unwrap()
        .map(|n| n.id.clone())
        .collect();
    for chunk in ids.chunks(500) {
        let ops: Vec<Operation> = chunk
            .iter()
            .map(|id| Operation::UpdateNode {
                id: id.clone(),
                props: json!({"name":"stamped"}),
                merge: false,
            })
            .collect();
        state.apply(&ops).unwrap();
    }
    let (stamped, after) = state.stamp_stats();
    let per = (after - before) as f64 / ids.len() as f64;
    println!(
        "\nStamps: {} written nodes, +{per:.1} B per node written once; {stamped} stamped \
         nodes hold {:.2} MiB. Slot size {} B.",
        ids.len(),
        mib(after),
        ProjectState::slot_size()
    );
}
