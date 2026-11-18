//! Cases of `node_memory_writes_bench`: one request shape per Task 41 benchmark row,
//! with targets picked from the generated fixture (`generate.mjs`: pages, eight masters
//! on `page_library`, root frames `root_N` with sections, texts, boxes and instances).
pub mod measure;

use fagent_storage::*;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Ids the cases touch.
pub struct Targets {
    parent: HashMap<String, Option<String>>,
    /// A text child of `root_8`, used by no instance: the small props edit.
    pub text: String,
    /// The frame child of `root_5` with the largest subtree, moved between roots.
    pub section: String,
    /// Authored instances of `master_0` (users of the master edit).
    pub master_users: usize,
    /// Authored instances of `master_1` (users of the token edit).
    pub token_users: usize,
}

impl Targets {
    pub fn new(s: &Snapshot) -> Self {
        let parent: HashMap<String, Option<String>> = s
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.parent_id.clone()))
            .collect();
        let child_of = |id: &str, root: &str| parent[id].as_deref() == Some(root);
        let text = s
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Text && child_of(&n.id, "root_8"))
            .map(|n| n.id.clone())
            .min()
            .expect("a text child of root_8");
        let mut size: HashMap<&str, usize> = HashMap::new();
        for node in &s.nodes {
            let mut id = node.id.as_str();
            while let Some(Some(p)) = parent.get(id) {
                if p == "root_5" {
                    *size.entry(id).or_default() += 1;
                    break;
                }
                id = p;
            }
        }
        let section = s
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Frame && child_of(&n.id, "root_5"))
            .map(|n| (size.get(n.id.as_str()).copied().unwrap_or(0), n.id.clone()))
            .max()
            .expect("a frame child of root_5")
            .1;
        let users = |master: &str| {
            s.nodes
                .iter()
                .filter(|n| n.props.get("componentId").and_then(Value::as_str) == Some(master))
                .count()
        };
        Self {
            text,
            section,
            master_users: users("master_0"),
            token_users: users("master_1"),
            parent,
        }
    }

    /// The page `id` sits on.
    pub fn page(&self, id: &str) -> String {
        let mut id = id;
        while let Some(Some(p)) = self.parent.get(id) {
            id = p;
        }
        id.to_owned()
    }
}

/// One benchmark row: the ops of run `n` and the pages it scopes.
pub struct Case {
    pub name: String,
    pub pages: Vec<String>,
    pub ops: Box<dyn Fn(usize) -> Value>,
}

/// Unmeasured ops that prepare the token case: a color token used by a child of
/// `master_1`, so a token edit reaches every instance of that master.
pub fn token_setup() -> Value {
    json!([{"op":"upsert_token","token":{"id":"bench_ink","name":"Bench ink","type":"color","value":"#000000"}},
        {"op":"update_node","id":"master_1_icon","props":{"fill":{"tokenId":"bench_ink"}}}])
}

pub fn cases(t: &Targets) -> Vec<Case> {
    let text = t.text.clone();
    let section = t.section.clone();
    let note_anchor = t.text.clone();
    let mut reparent_pages = vec![t.page("root_5"), t.page("root_6")];
    reparent_pages.dedup();
    vec![
        Case {
            name: "props edit".into(),
            pages: vec![t.page(&text)],
            ops: Box::new(
                move |n| json!([{"op":"update_node","id":text,"props":{"name":format!("w{n}")}}]),
            ),
        },
        Case {
            name: format!("master edit ({} instances)", t.master_users),
            pages: vec!["page_library".into()],
            ops: Box::new(
                |n| json!([{"op":"update_node","id":"master_0_title","props":{"name":format!("w{n}")}}]),
            ),
        },
        Case {
            name: "reparent".into(),
            pages: reparent_pages,
            ops: Box::new(move |n| {
                let parent = ["root_6", "root_5"][n % 2];
                json!([{"op":"reparent_node","id":section,"parentId":parent}])
            }),
        },
        Case {
            name: format!("token edit ({} instances)", t.token_users),
            pages: vec!["page_library".into()],
            ops: Box::new(|n| {
                json!([{"op":"upsert_token","token":{"id":"bench_ink","name":"Bench ink","type":"color",
                    "value":format!("#{:06x}", (n * 7919) % 0xffffff)}}])
            }),
        },
        Case {
            name: "note create".into(),
            pages: vec![t.page(&note_anchor)],
            ops: Box::new(
                move |n| json!([{"op":"create_note","id":format!("bench_note_{n}"),"nodeId":note_anchor,"text":format!("Note {n}")}]),
            ),
        },
        Case {
            name: "page create".into(),
            pages: vec!["page_library".into()],
            ops: Box::new(|n| {
                json!([{"op":"create_node","id":format!("bench_page_{n}"),"kind":"page","parentId":null,
                    "props":{"name":format!("Bench {n}")}}])
            }),
        },
    ]
}
