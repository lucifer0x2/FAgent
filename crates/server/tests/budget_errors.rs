//! FB-13/FB-49: pixel budget failures state the limit; FB-16/FB-48: revision failures name the
//! latest revision. Runs on the native backend, so no render worker is needed.
#[allow(dead_code)]
mod native_support;
use native_support::Server;
use serde_json::{json, Value};

/// A fixed 1440×6000 root, a hug root holding a 13,000 px tall box and a 2100×2100 node.
fn design() -> Value {
    json!([
        {"op":"create_node","id":"page","kind":"page","parentId":null,"props":{}},
        {"op":"create_node","id":"long","kind":"frame","parentId":"page","props":{"width":1440,"height":6000,"fill":"#ffffff"}},
        {"op":"create_node","id":"huge","kind":"rectangle","parentId":"long","props":{"width":2100,"height":2100,"fill":"#ef4444"}},
        {"op":"create_node","id":"tall","kind":"frame","parentId":"page","props":{"width":1440,"height":"hug","layout":{"mode":"column"}}},
        {"op":"create_node","id":"tower","kind":"rectangle","parentId":"tall","props":{"width":100,"height":13000,"fill":"#ef4444"}},
        {"op":"create_node","id":"mid","kind":"frame","parentId":"page","props":{"width":1440,"height":"hug","layout":{"mode":"column"}}},
        {"op":"create_node","id":"block","kind":"rectangle","parentId":"mid","props":{"width":100,"height":6000,"fill":"#ef4444"}},
    ])
}

fn render(frame: &str, width: u32, height: u32, scale: f64) -> Value {
    json!({"projectId":"","revision":1,"frameId":frame,"viewport":{"width":width,"height":height},"scale":scale})
}

async fn enqueue(s: &Server, p: &str, mut args: Value) -> Value {
    args["projectId"] = json!(p);
    s.call("render_frame", args).await
}

async fn rendered(s: &Server, p: &str, args: Value) -> Value {
    let pending = enqueue(s, p, args).await;
    assert!(pending["jobId"].is_string(), "{pending}");
    s.wait(pending["jobId"].as_str().unwrap()).await
}

#[tokio::test]
async fn render_and_export_budget_errors_state_the_numbers() {
    let s = Server::start().await;
    let p = s.project().await;
    assert_eq!(s.batch(&p, 0, design()).await["committedRevision"], 1);

    let scaled = enqueue(&s, &p, render("long", 1440, 6000, 2.0)).await;
    let error = &scaled["error"];
    assert_eq!(error["code"], "PIXEL_BUDGET", "{scaled}");
    assert_eq!(error["maxScale"], 1.36);
    assert_eq!(
        (
            error["maxPixels"].as_u64(),
            error["requestedPixels"].as_u64()
        ),
        (Some(16_000_000), Some(34_560_000))
    );
    let tall = enqueue(&s, &p, render("long", 1440, 12000, 1.0)).await;
    let message = tall["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("17,280,000") && message.contains("16,000,000"),
        "{tall}"
    );

    // A hug root that grows past the budget at scale 1 fails in layout, the same way.
    let job = rendered(&s, &p, render("tall", 1440, 800, 1.0)).await;
    assert_eq!(job["error"]["code"], "PIXEL_BUDGET", "{job}");
    assert_eq!(job["error"]["maxPixels"], 16_000_000, "{job}");
    assert_eq!(job["error"]["maxHeight"], 11_111);
    // One that fits at scale 1 but not at the requested scale fails before painting.
    let job = rendered(&s, &p, render("mid", 1440, 800, 2.0)).await;
    assert_eq!(job["error"]["code"], "PIXEL_BUDGET", "{job}");
    assert_eq!(job["error"]["maxScale"], 1.36);
    assert_eq!(job["error"]["requestedPixels"], 34_560_000);

    let export = s
        .call(
            "export_nodes",
            json!({"projectId":p,"nodeIds":["huge"],"settings":[{"scale":4}]}),
        )
        .await;
    let error = &export["exports"][0]["error"];
    assert_eq!(error["code"], "PIXEL_BUDGET", "{export}");
    assert_eq!(error["maxScale"], 1.9);
    assert_eq!(error["requestedPixels"], 70_560_000);
}

#[tokio::test]
async fn revision_errors_name_the_latest_revision() {
    let s = Server::start().await;
    let p = s.project().await;
    assert_eq!(s.batch(&p, 0, design()).await["committedRevision"], 1);
    let check = |value: &Value| {
        let error = &value["error"];
        assert_eq!(error["code"], "REVISION_UNAVAILABLE", "{value}");
        assert_eq!(error["currentRevision"], 1, "{value}");
        assert!(error["message"]
            .as_str()
            .unwrap()
            .contains("only the latest revision (1)"));
    };
    let mut args = render("long", 320, 200, 1.0);
    args["revision"] = json!(0);
    check(&enqueue(&s, &p, args).await);
    check(
        &s.call(
            "export_nodes",
            json!({"projectId":p,"nodeIds":["huge"],"revision":0}),
        )
        .await,
    );
    check(
        &s.call(
            "get_subtree",
            json!({"projectId":p,"nodeId":"long","revision":0}),
        )
        .await,
    );
    check(
        &s.get(&format!(
            "/api/v1/projects/{p}/scenes?pageId=page&revision=0"
        ))
        .await,
    );
}
