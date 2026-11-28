//! Task 44: size and layout overrides (FB-12, FB-30), override paths through nested
//! instances (FB-28), cross-axis `fill` in a stretching stack (FB-22) and `@set
//! overrides={…}` merging per descendant (FB-9 a), on the native compute path.
#[allow(dead_code)]
mod native_support;
use native_support::Server;
use serde_json::{json, Value};

fn op(id: &str, kind: &str, parent: &str, props: Value) -> Value {
    let parent = (!parent.is_empty()).then_some(parent);
    json!({"op":"create_node","id":id,"kind":kind,"parentId":parent,"props":props})
}

fn text(id: &str, parent: &str, value: &str) -> Value {
    op(
        id,
        "text",
        parent,
        json!({"text":value,"fontId":"font_bundled","fontSize":14,"lineHeight":20}),
    )
}

fn master(id: &str, x: u32, props: Value) -> Value {
    let mut props = props;
    props["component"] = json!({"name":id});
    props["x"] = json!(x);
    op(id, "frame", "page", props)
}

/// Masters on the page; instances in a 400×600 column root `frame`.
fn design() -> Value {
    json!([
        op("page", "page", "", json!({})),
        op(
            "frame",
            "frame",
            "page",
            json!({"width":400,"height":600,"layout":{"mode":"column"}})
        ),
        // FB-12/FB-30: a button whose inner row is overridden.
        master(
            "btn",
            500,
            json!({"width":"hug","height":40,"layout":{"mode":"row"}})
        ),
        op(
            "inner",
            "frame",
            "btn",
            json!({"width":100,"height":40,"layout":{"mode":"row","padding":[0,6],"gap":2}})
        ),
        op("dot", "rectangle", "inner", json!({"width":10,"height":10})),
        op(
            "dot2",
            "rectangle",
            "inner",
            json!({"width":10,"height":10})
        ),
        // FB-28: card contains an instance of leaf; trio contains an instance of card.
        master("leaf", 700, json!({"width":80,"height":20})),
        text("t", "leaf", "Leaf"),
        master("card", 800, json!({"width":80,"height":20})),
        op(
            "b",
            "instance",
            "card",
            json!({"componentId":"leaf","overrides":{"t":{"text":"Own","visible":true}}})
        ),
        master("trio", 900, json!({"width":80,"height":20})),
        op("a", "instance", "trio", json!({"componentId":"card"})),
        // FB-22: a fixed 64×120 master and a free master without a height.
        master("tile", 1000, json!({"width":64,"height":120})),
        master("loose", 1100, json!({"width":40})),
        op(
            "i_btn",
            "instance",
            "frame",
            json!({"componentId":"btn","layout":{"padding":[0,20]},
            "overrides":{"inner":{"width":164,"layout":{"justify":"center"}}}})
        ),
        op(
            "i_card",
            "instance",
            "frame",
            json!({"componentId":"card","overrides":{"b/t":{"text":"Deep"}}})
        ),
        op(
            "i_trio",
            "instance",
            "frame",
            json!({"componentId":"trio","overrides":{"a/b/t":{"text":"Deeper"}}})
        ),
        op(
            "row",
            "frame",
            "frame",
            json!({"width":400,"layout":{"mode":"row","align":"stretch","gap":4}})
        ),
        op("tall", "rectangle", "row", json!({"width":50,"height":300})),
        op(
            "i_tile",
            "instance",
            "row",
            json!({"componentId":"tile","height":"fill"})
        ),
        op(
            "i_loose",
            "instance",
            "row",
            json!({"componentId":"loose","height":"fill"})
        ),
        op("plain", "frame", "row", json!({"width":30,"height":"fill"})),
    ])
}

async fn fixture(s: &Server) -> String {
    let p = s.project().await;
    let result = s.batch(&p, 0, design()).await;
    assert_eq!(result["committedRevision"], 1, "{result}");
    p
}

async fn scene(s: &Server, p: &str, revision: u64) -> Value {
    let args = json!({"projectId":p,"revision":revision,"frameId":"frame",
        "viewport":{"width":400,"height":600}});
    let pending = s.call("render_frame", args).await;
    assert!(pending["jobId"].is_string(), "{pending}");
    let job = s.wait(pending["jobId"].as_str().unwrap()).await;
    assert_eq!(job["status"], "ready", "{job}");
    job["scene"].clone()
}

fn node<'a>(scene: &'a Value, id: &str) -> &'a Value {
    scene["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == id)
        .unwrap_or_else(|| panic!("no scene node {id}"))
}

fn bounds(scene: &Value, id: &str) -> [f64; 4] {
    ["x", "y", "width", "height"].map(|k| node(scene, id)["bounds"][k].as_f64().unwrap())
}

fn line(scene: &Value, id: &str) -> String {
    node(scene, id)["lines"][0]["text"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn rejected(s: &Server, p: &str, ops: Value) -> Value {
    let result = s.batch(p, 1, ops).await;
    assert!(result["error"].is_object(), "{result}");
    result["error"].clone()
}

#[tokio::test]
async fn size_layout_paths_and_stretch_reach_the_scene() {
    let s = Server::start().await;
    let p = fixture(&s).await;
    let scene = scene(&s, &p, 1).await;

    // FB-12/FB-30: the overridden width and justify apply; padding and gap are kept.
    let root = bounds(&scene, "i_btn");
    let inner = bounds(&scene, "i_btn/inner");
    assert_eq!(inner[2], 164., "{inner:?}");
    // The instance's own layout.padding moves the master's children.
    assert_eq!(inner[0] - root[0], 20., "{root:?} {inner:?}");
    assert_eq!(root[2], 204.);
    // Centered 10 + 2 + 10 content in 164 − 12 padding: starts at 6 + (152 − 22) / 2.
    let dot = bounds(&scene, "i_btn/dot");
    let dot2 = bounds(&scene, "i_btn/dot2");
    assert_eq!(dot[0] - inner[0], 71., "{dot:?}");
    assert_eq!(dot2[0] - dot[0], 12., "gap kept");

    // FB-28: two- and three-level paths; the outer text wins, the nested own key stays.
    assert_eq!(line(&scene, "i_card/b/t"), "Deep");
    assert_eq!(line(&scene, "i_trio/a/b/t"), "Deeper");

    // FB-22: a cross-axis fill stretches to the tallest sibling of the hug row.
    for id in ["i_tile", "i_loose", "plain"] {
        assert_eq!(bounds(&scene, id)[3], 300., "{id}");
    }
    assert_eq!(bounds(&scene, "row")[3], 300.);
}

#[tokio::test]
async fn override_paths_and_sizes_are_validated() {
    let s = Server::start().await;
    let p = fixture(&s).await;
    // A dangling path names the field and suggests the path that exists.
    let error = rejected(
        &s,
        &p,
        json!([{"op":"update_node","id":"i_card","props":{"overrides":{"b/tt":{"text":"x"}}}}]),
    )
    .await;
    assert_eq!(error["field"], "props.overrides.b/tt", "{error}");
    assert_eq!(error["suggestion"], "b/t", "{error}");
    // A segment followed by more must be an instance.
    let error = rejected(
        &s,
        &p,
        json!([{"op":"update_node","id":"i_btn","props":{"overrides":{"inner/dot":{"visible":false}}}}]),
    )
    .await;
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("not an instance"),
        "{error}"
    );
    // Fill inside a hug axis is still rejected under the override's field.
    let error = rejected(
        &s,
        &p,
        json!([{"op":"update_node","id":"i_btn","props":{"overrides":{"inner":{"width":"fill"}}}}]),
    )
    .await;
    assert_eq!(error["field"], "props.overrides.inner.width", "{error}");
    // Deleting the nested master's text leaves the outer path dangling: the commit fails
    // (once `b`'s own override of `t`, which fails first, is gone).
    let clear = json!([{"op":"update_node","id":"b","merge":true,"props":{"overrides":null}}]);
    assert_eq!(s.batch(&p, 1, clear).await["committedRevision"], 2);
    let result = s
        .batch(&p, 2, json!([{"op":"delete_subtree","id":"t"}]))
        .await;
    assert_eq!(result["error"]["field"], "props.overrides.b/t", "{result}");
    assert_eq!(result["error"]["nodeId"], "i_card", "{result}");
}

#[tokio::test]
async fn set_overrides_merges_per_descendant() {
    let s = Server::start().await;
    let p = fixture(&s).await;
    let source = r#"@set i_card overrides={"b/t":{"visible":false}}"#;
    let args = json!({"projectId":p,"expectedRevision":1,"idempotencyKey":"set","source":source});
    let result = s.call("apply_batch", args).await;
    assert_eq!(result["committedRevision"], 2, "{result}");
    let read = s
        .call("get_subtree", json!({"projectId":p,"nodeId":"i_card"}))
        .await;
    assert_eq!(
        read["nodes"][0]["props"]["overrides"],
        json!({"b/t":{"text":"Deep","visible":false}})
    );
}
