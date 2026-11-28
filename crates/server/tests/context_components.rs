//! FB-33: `get_design_context` pages master definitions as `components` entries and lists only
//! the masters an instance within `depth` needs, so a frame whose definitions together exceed
//! the 64 KiB page budget still reads at `depth:0, compact:true`.
#[allow(dead_code)]
mod support;
use serde_json::{json, Value};
use support::Server;

const MASTERS: usize = 30;
const TEXTS: usize = 30;

fn create(id: &str, kind: &str, parent: &str, props: Value) -> Value {
    json!({"op":"create_node","id":id,"kind":kind,"parentId":parent,"props":props})
}

fn text(value: String) -> Value {
    json!({"text":value,"fontId":"font_bundled","fontSize":14,"lineHeight":20,"width":320,"height":20,"fill":"#223344"})
}

/// 30 masters of 31 nodes each (`m0` also holds an instance of `m1`), and a frame `screen`
/// with an `m0` instance at depth 1 and one instance of every master at depth 2 inside five
/// sections, plus filler rectangles: about 1,000 nodes in all.
fn operations() -> Vec<Value> {
    let mut ops =
        vec![json!({"op":"create_node","id":"page","kind":"page","parentId":null,"props":{}})];
    for m in 0..MASTERS {
        let master = format!("m{m}");
        ops.push(create(&master, "frame", "page", json!({"component":{"name":format!("Master {m}")},"width":360,"height":"hug","layout":{"mode":"column","gap":4}})));
        for t in 0..TEXTS {
            let line = format!(
                "Master {m} line {t}: a sentence long enough to give each definition weight"
            );
            ops.push(create(&format!("m{m}-t{t}"), "text", &master, text(line)));
        }
    }
    ops.push(create(
        "m0-nested",
        "instance",
        "m0",
        json!({"componentId":"m1"}),
    ));
    ops.push(create(
        "screen",
        "frame",
        "page",
        json!({"width":1440,"height":"hug","layout":{"mode":"column","gap":8}}),
    ));
    ops.push(create(
        "top",
        "instance",
        "screen",
        json!({"componentId":"m0"}),
    ));
    for s in 0..5 {
        let section = format!("section{s}");
        ops.push(create(
            &section,
            "frame",
            "screen",
            json!({"width":"fill","height":"hug","layout":{"mode":"column","gap":8}}),
        ));
        for m in (s..MASTERS).step_by(5) {
            ops.push(create(
                &format!("use{m}"),
                "instance",
                &section,
                json!({"componentId":format!("m{m}")}),
            ));
        }
        for r in 0..8 {
            ops.push(create(
                &format!("section{s}-r{r}"),
                "rectangle",
                &section,
                json!({"width":40,"height":4,"fill":"#dce1ea"}),
            ));
        }
    }
    ops
}

async fn fixture(s: &Server) -> String {
    let project = s.project().await;
    let mut revision = 0;
    for (i, chunk) in operations().chunks(400).enumerate() {
        let result = s
            .batch(&project, revision, &format!("fixture-{i}"), json!(chunk))
            .await;
        revision = result["committedRevision"]
            .as_u64()
            .unwrap_or_else(|| panic!("{result}"));
    }
    project
}

/// Every page of one read, failing on an error or a page over 64 KiB.
async fn pages(s: &Server, mut args: Value) -> Vec<Value> {
    let mut pages = Vec::new();
    loop {
        let page = s.call("get_design_context", args.clone()).await;
        assert!(page.get("error").is_none(), "{page}");
        assert!(serde_json::to_vec(&page).unwrap().len() <= 65_536);
        let next = page["nextCursor"].clone();
        pages.push(page);
        if next.is_null() {
            return pages;
        }
        assert!(pages.len() < 100, "unexpected page count");
        args["cursor"] = next;
    }
}

fn components(pages: &[Value]) -> Vec<Value> {
    pages
        .iter()
        .flat_map(|page| page["components"].as_array().cloned().unwrap_or_default())
        .collect()
}

fn root_ids(components: &[Value]) -> Vec<String> {
    components
        .iter()
        .map(|c| c["rootId"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn large_frame_reads_at_depth_zero_and_pages_every_definition() {
    let s = Server::start().await;
    let project = fixture(&s).await;
    // Each operation creates one node.
    let total = operations().len();
    assert!((950..=1_100).contains(&total), "{total} nodes");

    // depth 0 lists the frame alone and no instance, so no definition is needed.
    let shallow = json!({"projectId":project,"frameId":"screen","depth":0,"compact":true});
    let shallow = pages(&s, shallow).await;
    assert_eq!(shallow[0]["nodes"].as_array().unwrap().len(), 1);
    assert!(components(&shallow).is_empty());

    // depth 1 reaches the `m0` instance; `m1` is listed because `m0` holds an instance of it.
    let one = json!({"projectId":project,"frameId":"screen","depth":1,"compact":true});
    assert_eq!(root_ids(&components(&pages(&s, one).await)), ["m0", "m1"]);

    // The whole frame needs every master: together they exceed one page, so they span pages,
    // each exactly once and complete.
    let full = json!({"projectId":project,"frameId":"screen","compact":true});
    let full = pages(&s, full).await;
    let definitions = components(&full);
    assert!(
        full.iter()
            .filter(|p| p.get("components").is_some())
            .count()
            > 1,
        "definitions should span pages"
    );
    let mut expected: Vec<_> = (0..MASTERS).map(|m| format!("m{m}")).collect();
    expected.sort();
    assert_eq!(root_ids(&definitions), expected);
    for definition in &definitions {
        let id = definition["rootId"].as_str().unwrap();
        let count = TEXTS + 1 + usize::from(id == "m0");
        assert_eq!(definition["nodes"].as_array().unwrap().len(), count, "{id}");
        assert_eq!(definition["nodes"][0]["id"], id);
    }
}

#[tokio::test]
async fn oversized_definition_names_its_field_item_and_size() {
    let s = Server::start().await;
    let project = fixture(&s).await;
    let tight = json!({"projectId":project,"frameId":"screen","depth":2,"budget":4096});
    let error = s.call("get_design_context", tight).await["error"].clone();
    assert_eq!(error["code"], "CONTEXT_ITEM_TOO_LARGE", "{error}");
    assert_eq!(error["field"], "components", "{error}");
    assert_eq!(error["itemId"], "m0", "{error}");
    let bytes = error["bytes"].as_u64().unwrap();
    assert!(bytes > 4096, "{error}");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains(&bytes.to_string()), "{message}");
    assert!(message.contains("get_subtree"), "{message}");
    // 4096 is below the maximum, so a larger budget is a possible action.
    assert!(message.contains("raise budget"), "{message}");
}
