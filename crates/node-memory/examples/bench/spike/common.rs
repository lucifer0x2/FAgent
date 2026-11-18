//! Fixture loading, memory rows and the write workload shared by the three options.
use crate::load::limits;
use crate::memory::{self, sample, Sample};
use crate::writes::create_op;
use fagent_node_memory::Operation;
use fagent_scene_input::NodeKind;
use fagent_scene_input::Snapshot;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;

/// Reads and parses the fixture and drops the file bytes; returns the snapshot and the
/// memory sample with only the snapshot alive.
pub fn snapshot(path: &Path) -> (Snapshot, Sample) {
    let bytes = std::fs::read(path).expect("fixture");
    let snapshot: Snapshot = serde_json::from_slice(&bytes).expect("fixture JSON");
    drop(bytes);
    (snapshot, sample())
}

pub fn max_nodes(snapshot: &Snapshot) -> usize {
    limits(snapshot.nodes.len()).max_nodes
}

/// Memory rows after load; `B/node` divides the process footprint by authored nodes,
/// like the baseline `after load (snapshot moved in)` row.
pub fn memory_rows(option: &str, parsed: Sample, loaded: Sample, authored: usize) {
    let per = |mib: f64| mib * 1024.0 * 1024.0 / authored as f64;
    memory::header();
    memory::row("after parse (snapshot only)", parsed, "");
    let note = format!(
        "{option}: RSS {:.0} B/node, footprint {:.0} B/node",
        per(loaded.rss),
        per(loaded.footprint)
    );
    memory::row("after load (snapshot moved in)", loaded, &note);
    memory::peak_row("peak through load", loaded);
    println!("\n{authored} authored nodes\n");
}

/// The write workload: (row name, ops per run, note). Ids are fresh per run.
pub struct Workload {
    pub name: &'static str,
    pub ops: Box<dyn Fn(usize) -> Vec<Operation>>,
    pub note: &'static str,
    /// Run cap: the wide-frame renumber is too slow on (a) for the full run count.
    pub cap: usize,
}

pub fn workloads(text: String, section: String) -> Vec<Workload> {
    vec![
        Workload {
            name: "commit: create node (append)",
            ops: Box::new(|run| vec![create_op(format!("spike_new_{run}"), "root_7", None)]),
            note: "",
            cap: usize::MAX,
        },
        Workload {
            name: "commit: create at index 0 of wide frame",
            ops: Box::new(|run| {
                vec![create_op(
                    format!("spike_front_{run}"),
                    "frame_wide",
                    Some(0),
                )]
            }),
            note: "renumbers ~14k siblings",
            cap: 5,
        },
        Workload {
            name: "commit: reparent subtree across roots",
            ops: Box::new(move |run| {
                vec![Operation::ReparentNode {
                    id: section.clone(),
                    parent_id: ["root_6", "root_5"][run % 2].into(),
                    index: None,
                }]
            }),
            note: "",
            cap: usize::MAX,
        },
        Workload {
            name: "commit: delete subtree (normal root frame)",
            ops: Box::new(|run| {
                vec![Operation::DeleteSubtree {
                    id: format!("root_{}", 100 + run),
                }]
            }),
            note: "",
            cap: usize::MAX,
        },
        Workload {
            name: "commit: update props x100 (100-op batch)",
            ops: Box::new(move |run| (0..100).map(|i| update(&text, run * 100 + i)).collect()),
            note: "same node, 100 merge patches",
            cap: usize::MAX,
        },
    ]
}

/// A merge patch whose value differs per `n`, so every run really changes the node.
pub fn update(id: &str, n: usize) -> Operation {
    Operation::UpdateNode {
        id: id.into(),
        props: json!({ "text": format!("changed {n}") }),
        merge: true,
    }
}

/// Ids the workload touches, picked from the snapshot so every option uses the same:
/// a text child of `root_8` and the frame child of `root_5` with the largest subtree.
pub fn targets(snapshot: &Snapshot) -> (String, String) {
    let text = snapshot
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Text && n.parent_id.as_deref() == Some("root_8"))
        .map(|n| n.id.clone())
        .min()
        .expect("text in root_8");
    let parent: HashMap<&str, &str> = snapshot
        .nodes
        .iter()
        .filter_map(|n| n.parent_id.as_deref().map(|p| (n.id.as_str(), p)))
        .collect();
    let mut size: HashMap<&str, usize> = HashMap::new();
    for node in &snapshot.nodes {
        let mut id = node.id.as_str();
        while let Some(p) = parent.get(id) {
            if *p == "root_5" {
                *size.entry(id).or_default() += 1;
                break;
            }
            id = p;
        }
    }
    let frames = snapshot
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Frame && n.parent_id.as_deref() == Some("root_5"));
    let section = frames
        .map(|n| (size.get(n.id.as_str()).copied().unwrap_or(0), n.id.clone()))
        .max()
        .expect("frame in root_5")
        .1;
    (text, section)
}
