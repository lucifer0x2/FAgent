//! The read mix of the differential, derived from the project's structure: every
//! root frame, nested frames, instances, virtual instance children, pages, unknown
//! and invalid targets, paging with small limits, and reads memory never serves.
use super::Pair;
use fagent_storage::{Node, NodeKind, Snapshot};
use serde_json::{json, Value};
use std::collections::HashMap;

/// Caps per category keep the 5k run short; root frames are never capped.
pub(super) struct Caps {
    pub nested: usize,
    pub instances: usize,
    pub virtuals: usize,
}

pub(super) struct Targets {
    pub pages: Vec<String>,
    pub roots: Vec<String>,
    /// Root frames that are not component masters (links and notes go there).
    pub plain: Vec<String>,
    pub nested: Vec<String>,
    pub instances: Vec<String>,
    pub leaves: Vec<String>,
}

impl Targets {
    pub fn of(snapshot: &Snapshot, caps: &Caps) -> Self {
        let kind: HashMap<&str, &NodeKind> = snapshot
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), &n.kind))
            .collect();
        let parent_kind = |n: &Node| n.parent_id.as_deref().map(|p| kind[p]);
        let ids = |f: &dyn Fn(&Node) -> bool, cap: usize| -> Vec<String> {
            let all: Vec<_> = snapshot.nodes.iter().filter(|n| f(n)).collect();
            let step = (all.len() / cap.max(1)).max(1);
            all.iter()
                .step_by(step)
                .take(cap)
                .map(|n| n.id.clone())
                .collect()
        };
        let frame = |n: &Node| n.kind == NodeKind::Frame;
        Self {
            pages: ids(&|n| n.kind == NodeKind::Page, usize::MAX),
            roots: ids(
                &|n| frame(n) && parent_kind(n) == Some(&NodeKind::Page),
                usize::MAX,
            ),
            plain: ids(
                &|n| {
                    frame(n)
                        && parent_kind(n) == Some(&NodeKind::Page)
                        && n.props.get("component").is_none()
                },
                usize::MAX,
            ),
            nested: ids(
                &|n| frame(n) && parent_kind(n) != Some(&NodeKind::Page),
                caps.nested,
            ),
            instances: ids(&|n| n.kind == NodeKind::Instance, caps.instances),
            leaves: ids(&|n| n.kind == NodeKind::Text, 20),
        }
    }
}

/// Every root frame, with paging, formats and expansion; returns virtual ids seen.
pub(super) async fn roots(pair: &mut Pair, t: &Targets, cap: usize) -> Vec<String> {
    let mut virtuals = Vec::new();
    for id in &t.roots {
        let frame = json!({"frameId": id});
        pair.walk("get_design_context", frame.clone()).await;
        pair.walk(
            "get_design_context",
            json!({"frameId":id,"limit":25,"budget":16000}),
        )
        .await;
        pair.one("get_design_context", json!({"frameId":id,"compact":true}))
            .await;
        pair.one(
            "get_design_context",
            json!({"frameId":id,"nodeFormat":"rows","notes":"all"}),
        )
        .await;
        let expanded = pair
            .walk(
                "get_design_context",
                json!({"frameId":id,"instances":"expand","limit":60}),
            )
            .await;
        for page in &expanded {
            for node in page["nodes"].as_array().into_iter().flatten() {
                let vid = node["id"].as_str().unwrap_or_default();
                if vid.contains('/') && virtuals.len() < cap {
                    virtuals.push(vid.to_owned());
                }
            }
        }
        pair.one("get_subtree", json!({"nodeId":id,"depth":2}))
            .await;
        pair.walk("get_outline", json!({"nodeId":id,"limit":50}))
            .await;
        pair.walk(
            "find_nodes",
            json!({"frameId":id,"kinds":["text"],"limit":20}),
        )
        .await;
        pair.walk(
            "find_nodes",
            json!({"frameId":id,"kinds":["text"],"limit":20,"instances":"expand"}),
        )
        .await;
        pair.one(
            "find_nodes",
            json!({"nodeId":id,"nameContains":"e","limit":10}),
        )
        .await;
    }
    virtuals
}

/// Nested frames, instances, virtual children, pages and leaves.
pub(super) async fn inner(pair: &mut Pair, t: &Targets, virtuals: &[String]) {
    for id in &t.nested {
        pair.walk("get_design_context", json!({"frameId": id}))
            .await;
        pair.one("get_subtree", json!({"nodeId": id})).await;
        pair.one("find_nodes", json!({"nodeId":id,"kinds":["rectangle"]}))
            .await;
    }
    for id in &t.instances {
        pair.one("get_subtree", json!({"nodeId": id})).await;
        pair.one("get_design_context", json!({"frameId": id})).await;
        pair.one("find_nodes", json!({"nodeId": id})).await;
    }
    for id in virtuals {
        pair.one("get_subtree", json!({"nodeId": id})).await;
        pair.one("get_outline", json!({"nodeId": id})).await;
        pair.one("find_nodes", json!({"nodeId": id})).await;
    }
    for id in &t.pages {
        pair.walk("get_outline", json!({"nodeId":id,"depth":1,"limit":40}))
            .await;
        pair.walk(
            "find_nodes",
            json!({"nodeId":id,"kinds":["frame"],"limit":30}),
        )
        .await;
    }
    for id in &t.leaves {
        pair.one("get_subtree", json!({"nodeId": id})).await;
        pair.one("get_design_context", json!({"frameId": id})).await;
    }
}

/// Unknown and invalid scopes, and reads that keep today's path.
pub(super) async fn edges(pair: &mut Pair, t: &Targets, revision: u64) {
    let root = t.plain.first().cloned().unwrap_or_default();
    let reads: [(&str, Value); 9] = [
        ("get_subtree", json!({"nodeId":"missing"})),
        ("get_design_context", json!({"frameId":"missing"})),
        ("find_nodes", json!({"nodeId":"missing/child"})),
        ("get_outline", json!({"nodeId":root,"frameId":root})),
        (
            "get_design_context",
            json!({"frameId":root,"revision":revision}),
        ),
        (
            "get_design_context",
            json!({"frameId":root,"revision":revision + 7}),
        ),
        ("get_outline", json!({"limit":30})),
        ("find_nodes", json!({"textContains":"a","limit":15})),
        ("get_subtree", json!({"nodeId":root,"cursor":"bogus"})),
    ];
    for (name, args) in reads {
        pair.one(name, args).await;
    }
}

/// Small edits through both states: props, create, reorder, reparent, delete, notes.
pub(super) fn edits(t: &Targets) -> Vec<Value> {
    let leaf = t.leaves.first().cloned().unwrap_or_default();
    let root = t.plain.first().cloned().unwrap_or_default();
    let other = t.plain.get(1).cloned().unwrap_or(root.clone());
    vec![
        json!([{"op":"update_node","id":leaf,"props":{"name":"Edited by differential"}}]),
        json!([{"op":"create_node","id":"diff_box","kind":"rectangle","parentId":root,
                "props":{"width":12,"height":12,"links":[{"to":other}]}}]),
        json!([{"op":"reorder_node","id":"diff_box","index":0}]),
        json!([{"op":"reparent_node","id":"diff_box","parentId":other,"index":0}]),
        json!([{"op":"create_note","id":"diff_note","nodeId":"diff_box","text":"check"}]),
        json!([{"op":"delete_subtree","id":"diff_box"}]),
    ]
}
