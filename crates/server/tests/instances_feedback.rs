//! Instance authoring gaps from agent feedback, measured on the native compute and PNG
//! path: fill inside a free master (FB-23), root `states` (FB-40), `clip` on instances
//! and overrides (FB-51, FB-26) and text paint overrides (FB-31).
#[allow(dead_code)]
mod export_support;
#[allow(dead_code)]
mod native_support;
use export_support::{decode, pixel};
use native_support::Server;
use serde_json::{json, Value};

fn op(id: &str, kind: &str, parent: &str, props: Value) -> Value {
    let parent = (!parent.is_empty()).then_some(parent);
    json!({"op":"create_node","id":id,"kind":kind,"parentId":parent,"props":props})
}

/// Masters on the page, instances in a white 320×400 column root `frame`.
fn design() -> Value {
    json!([
        op("page", "page", "", json!({})),
        op(
            "frame",
            "frame",
            "page",
            json!({"width":320,"height":400,"fill":"#ffffff",
            "layout":{"mode":"column","padding":20,"gap":20}})
        ),
        // FB-23: a free 24×55 master with fill-sized children.
        op(
            "icon",
            "frame",
            "page",
            json!({"component":{"name":"Icon"},"x":400,"width":24,"height":55})
        ),
        op(
            "bg",
            "rectangle",
            "icon",
            json!({"width":"fill","height":"fill","fill":"#00ff00"})
        ),
        // FB-40: a plain button master.
        op(
            "button",
            "frame",
            "page",
            json!({"component":{"name":"Button"},"x":500,"width":60,"height":30,"fill":"#eeeeee"})
        ),
        // FB-51/FB-26: a child past the master edge and a clipping inner frame.
        op(
            "card",
            "frame",
            "page",
            json!({"component":{"name":"Card"},"x":600,"width":40,"height":40,"fill":"#eeeeee"})
        ),
        op(
            "over",
            "rectangle",
            "card",
            json!({"x":30,"y":10,"width":30,"height":20,"fill":"#ff0000"})
        ),
        op(
            "inner",
            "frame",
            "card",
            json!({"x":0,"y":0,"width":10,"height":10})
        ),
        op(
            "spill",
            "rectangle",
            "inner",
            json!({"x":5,"y":0,"width":20,"height":5,"fill":"#0000ff"})
        ),
        // FB-31: a text master.
        op(
            "label",
            "frame",
            "page",
            json!({"component":{"name":"Label"},"x":700,"width":120,"height":30})
        ),
        op(
            "caption",
            "text",
            "label",
            json!({"text":"Hello","fontId":"font_bundled","fontSize":14,"lineHeight":20})
        ),
        op("i_icon", "instance", "frame", json!({"componentId":"icon"})),
        op(
            "i_button",
            "instance",
            "frame",
            json!({"componentId":"button",
            "states":{"hover":{"fill":"#ff00ff"}}})
        ),
        op(
            "i_card",
            "instance",
            "frame",
            json!({"componentId":"card","clip":false,
            "overrides":{"inner":{"clip":false}}})
        ),
        op(
            "i_label",
            "instance",
            "frame",
            json!({"componentId":"label",
            "overrides":{"caption":{"textDecoration":"underline","textTransform":"uppercase",
                "letterSpacing":1,"textAlign":"center","textWrap":"nowrap"}}})
        ),
    ])
}

async fn render(s: &Server, p: &str, state: &str) -> Value {
    let args = json!({"projectId":p,"revision":1,"frameId":"frame",
        "viewport":{"width":320,"height":400},"state":state});
    let pending = s.call("render_frame", args).await;
    assert!(pending["jobId"].is_string(), "{pending}");
    let job = s.wait(pending["jobId"].as_str().unwrap()).await;
    assert_eq!(job["status"], "ready", "{job}");
    job
}

fn scene_node<'a>(job: &'a Value, id: &str) -> &'a Value {
    job["scene"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == id)
        .unwrap_or_else(|| panic!("no scene node {id}"))
}

fn bounds(node: &Value) -> [f64; 4] {
    ["x", "y", "width", "height"].map(|k| node["bounds"][k].as_f64().unwrap())
}

async fn png(s: &Server, job: &Value) -> (u32, u32, Vec<u8>) {
    let url = job["imageUrl"].as_str().unwrap();
    decode(
        &s.client
            .get(url)
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap(),
    )
}

async fn fixture(s: &Server) -> String {
    let p = s.project().await;
    let result = s.batch(&p, 0, design()).await;
    assert_eq!(result["committedRevision"], 1, "{result}");
    p
}

#[tokio::test]
async fn instance_root_states_clip_and_text_overrides_reach_the_scene_and_png() {
    let s = Server::start().await;
    let p = fixture(&s).await;
    let job = render(&s, &p, "default").await;

    // FB-23: fill children of a free master size against the instance, not the root.
    let icon = bounds(scene_node(&job, "i_icon"));
    assert_eq!(icon, [20., 20., 24., 55.]);
    assert_eq!(bounds(scene_node(&job, "i_icon/bg")), icon);

    // FB-51: clip:false on the instance root paints the child past its edge.
    let card = scene_node(&job, "i_card");
    assert_eq!(card["clip"], false, "{card}");
    // FB-26: clip:false on a descendant frame through an override.
    assert_eq!(scene_node(&job, "i_card/inner")["clip"], false);
    let [x, y, w, _] = bounds(card);
    let image = png(&s, &job).await;
    let past_edge = pixel(&image, (x + w + 10.) as u32, (y + 20.) as u32);
    assert_eq!(past_edge, [255, 0, 0, 255], "child stroke past the edge");
    let spill = pixel(&image, (x + 12.) as u32, (y + 2.) as u32);
    assert_eq!(
        spill,
        [0, 0, 255, 255],
        "descendant past an unclipped frame"
    );

    // FB-31: text paint overrides reach the master text child.
    let caption = scene_node(&job, "i_label/caption");
    assert_eq!(caption["decoration"]["line"], "underline", "{caption}");
    assert_eq!(caption["letterSpacing"], 1.);

    // FB-40: the instance root's own hover fill.
    let button = scene_node(&job, "i_button");
    assert_eq!(button["fill"], "#eeeeee", "{button}");
    let hover = render(&s, &p, "hover").await;
    assert_eq!(scene_node(&hover, "i_button")["fill"], "#ff00ff");
    let [x, y, _, _] = bounds(scene_node(&hover, "i_button"));
    let image = png(&s, &hover).await;
    assert_eq!(
        pixel(&image, (x + 5.) as u32, (y + 5.) as u32),
        [255, 0, 255, 255]
    );
}

async fn rejected(s: &Server, p: &str, props: Value) -> Value {
    let ops = json!([op("bad", "instance", "frame", props)]);
    let result = s.batch(p, 1, ops).await;
    assert!(result["error"].is_object(), "{result}");
    result["error"].clone()
}

#[tokio::test]
async fn instance_override_errors_name_the_field_and_the_accepted_form() {
    let s = Server::start().await;
    let p = fixture(&s).await;
    let error = rejected(
        &s,
        &p,
        json!({"componentId":"label","overrides":{"caption":{"textDecoration":"wavy"}}}),
    )
    .await;
    assert_eq!(
        error["field"], "props.overrides.caption.textDecoration",
        "{error}"
    );
    // FB-30: the master root is not an override target; its props live on the instance.
    let error = rejected(
        &s,
        &p,
        json!({"componentId":"button","overrides":{"button":{"fill":"#000000"}}}),
    )
    .await;
    assert_eq!(error["field"], "props.overrides.button", "{error}");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("set on the instance itself"), "{message}");
    assert!(message.contains("states"), "{message}");
    // Font keys stay outside the override list.
    let error = rejected(
        &s,
        &p,
        json!({"componentId":"label","overrides":{"caption":{"fontSize":20}}}),
    )
    .await;
    assert_eq!(
        error["field"], "props.overrides.caption.fontSize",
        "{error}"
    );
}
