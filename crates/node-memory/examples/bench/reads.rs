//! Read-path timings: lookups by id, children and subtrees, derived indexes.
use crate::support::{each, per_op, row};
use fagent_node_memory::ProjectState;

/// Up to 65,536 ids spread evenly over the project in page-then-preorder order, so
/// lookups hit the whole store without holding a copy of every id.
fn sample_ids(state: &ProjectState) -> Vec<String> {
    let stride = (state.len() / 65_536).max(1);
    state
        .pages()
        .flat_map(|p| state.subtree(&p.id).unwrap())
        .step_by(stride)
        .map(|n| n.id.clone())
        .collect()
}

pub fn reads(state: &ProjectState, runs: usize) {
    let ids = sample_ids(state);
    let sample = |i: usize| &ids[(i * 7919) % ids.len()];
    let lookups = |f: &dyn Fn(&str) -> bool| per_op(runs, 10_000, |i| f(sample(i)));
    row(
        "get node by id",
        lookups(&|id| state.node(id).is_some()),
        "",
    );
    let wide = state.children("frame_wide").unwrap().len();
    let children = each(
        runs,
        |_| (),
        |_| {
            let list = state.children("frame_wide").unwrap();
            list.map(|n| n.sort_key).sum::<i64>()
        },
    );
    row(
        "children of wide frame",
        children,
        &format!("{wide} children"),
    );
    let sub = each(
        runs,
        |_| (),
        |_| state.subtree("frame_wide").unwrap().count(),
    );
    row("subtree of wide frame", sub, &format!("{} nodes", wide + 1));
    let normal = state.subtree("root_10").unwrap().count();
    let sub = each(runs, |_| (), |_| state.subtree("root_10").unwrap().count());
    row(
        "subtree of normal root frame",
        sub,
        &format!("{normal} nodes"),
    );
    let root = lookups(&|id| state.root_frame_of(id).is_some());
    row("root_frame_of", root, "");
    row(
        "master_of",
        lookups(&|id| state.master_of(id).is_some()),
        "",
    );
    row("page_of", lookups(&|id| state.page_of(id).is_some()), "");
}

/// Scoped slice of a normal root frame (what an I2 read would hand to the readers).
pub fn slice(state: &ProjectState, runs: usize) {
    let nodes = state.slice("root_10").unwrap().nodes.len();
    let time = each(
        runs,
        |_| (),
        |_| state.slice("root_10").unwrap().nodes.len(),
    );
    row(
        "slice of normal root frame",
        time,
        &format!("{nodes} nodes incl. masters"),
    );
    let wide = each(
        runs,
        |_| (),
        |_| state.slice("frame_wide").unwrap().nodes.len(),
    );
    row("slice of wide frame", wide, "14k-node subtree");
}
