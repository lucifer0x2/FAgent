//! Write-path timings on the loaded project, one-operation batches of the stored
//! `Operation` input.
use crate::support::{each, row};
use fagent_node_memory::{Operation, ProjectState};
use fagent_scene_input::NodeKind;
use serde_json::json;

/// A nested frame under `root` with the largest subtree, and that size.
fn section(state: &ProjectState, root: &str) -> (String, usize) {
    let sections = state
        .children(root)
        .unwrap()
        .filter(|n| n.kind == NodeKind::Frame);
    let sized = sections.map(|n| (state.subtree(&n.id).unwrap().count(), n.id.clone()));
    let (size, id) = sized.max().expect("root has a nested frame");
    (id, size)
}

pub fn update_op(id: &str) -> Operation {
    Operation::UpdateNode {
        id: id.into(),
        props: json!({"text":"changed"}),
        merge: true,
    }
}

fn reparent(id: &str, parent: &str) -> Operation {
    Operation::ReparentNode {
        id: id.into(),
        parent_id: parent.into(),
        index: None,
    }
}

pub fn create_op(id: String, parent: &str, index: Option<usize>) -> Operation {
    Operation::CreateNode {
        id,
        kind: NodeKind::Rectangle,
        parent_id: Some(parent.into()),
        props: json!({}),
        index,
    }
}

pub fn text_in(state: &ProjectState, root: &str) -> String {
    let mut texts = state
        .children(root)
        .unwrap()
        .filter(|n| n.kind == NodeKind::Text);
    texts.next().unwrap().id.clone()
}

pub fn writes(state: &mut ProjectState, runs: usize) {
    let text = text_in(state, "root_7");
    let update = each(runs, |_| (), |_| state.apply(&[update_op(&text)]).unwrap());
    row("update props (merge patch, 1 op batch)", update, "");

    let (moved, size) = section(state, "root_5");
    let reparent_run =
        |state: &mut ProjectState, id: &str, to: &str| state.apply(&[reparent(id, to)]).unwrap();
    let time = each(
        runs,
        |run| ["root_6", "root_5"][run % 2],
        |to| reparent_run(state, &moved, to),
    );
    row(
        "reparent subtree across root frames",
        time,
        &format!("{size} nodes"),
    );
    let time = each(
        runs,
        |run| ["page_3", "page_1"][run % 2],
        |to| reparent_run(state, "frame_wide", to),
    );
    row(
        "reparent wide frame across pages",
        time,
        "page_of refresh of 14k+1 nodes",
    );

    let create = each(
        runs,
        |run| format!("bench_new_{run}"),
        |id| state.apply(&[create_op(id, "root_7", None)]).unwrap(),
    );
    row("create node (append)", create, "");
    let wide = state.children("frame_wide").unwrap().len();
    let append = each(
        runs,
        |run| format!("bench_tail_{run}"),
        |id| state.apply(&[create_op(id, "frame_wide", None)]).unwrap(),
    );
    row(
        "create node, append to wide frame",
        append,
        &format!("{wide} siblings checked"),
    );
    let middle = each(
        runs,
        |run| format!("bench_mid_{run}"),
        |id| {
            state
                .apply(&[create_op(id, "frame_wide", Some(wide / 2))])
                .unwrap()
        },
    );
    row(
        "create node at index n/2 of wide frame",
        middle,
        "renumbers ~7k siblings",
    );
    let front = each(
        runs,
        |run| format!("bench_front_{run}"),
        |id| {
            state
                .apply(&[create_op(id, "frame_wide", Some(0))])
                .unwrap()
        },
    );
    row(
        "create node at index 0 of wide frame",
        front,
        "renumbers ~14k siblings",
    );

    let mut sizes = Vec::new();
    let delete = each(
        runs,
        |run| format!("root_{}", 100 + run),
        |id| {
            sizes.push(state.subtree(&id).unwrap().count());
            state.apply(&[Operation::DeleteSubtree { id }]).unwrap()
        },
    );
    sizes.sort();
    let note = format!("median {} nodes", sizes[sizes.len() / 2]);
    row("delete subtree (normal root frame)", delete, &note);

    let rollback = each(
        runs,
        |_| (),
        |_| {
            let ops = [
                Operation::DeleteSubtree {
                    id: "frame_wide".into(),
                },
                Operation::ReorderNode {
                    id: "missing".into(),
                    index: 0,
                },
            ];
            state.apply(&ops).unwrap_err()
        },
    );
    row(
        "delete wide frame + rollback (failed batch)",
        rollback,
        "state restored",
    );
}
