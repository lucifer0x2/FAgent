//! FB-27 reproduction: a `free` frame written without `h` in a stretching row stores no
//! height and is laid out at the row's height, end to end (OCD source, storage, reads and a
//! native render's `inspect_layout`).
#[allow(dead_code)]
mod native_support;
use native_support::Server;
use serde_json::{json, Value};

const SOURCE: &str = "page#p
  frame#root w=320 col
    frame#row w=fill row gap=8 align=stretch
      frame#x w=24 free
        rect#dot x=4 y=4 8x8 fill=#111111
      rect#tall 10x55 fill=#ef4444
";

fn bounds(inspect: &Value, id: &str) -> Value {
    let nodes = inspect["nodes"].as_array().expect("inspect nodes");
    let node = nodes.iter().find(|n| n["id"] == id);
    node.unwrap_or_else(|| panic!("{id} in {inspect}"))["bounds"].clone()
}

#[tokio::test]
async fn a_free_frame_without_h_stretches_to_its_row() {
    let s = Server::start().await;
    let p = s.project().await;
    let args = json!({"projectId":p,"expectedRevision":0,"idempotencyKey":"fb27","source":SOURCE});
    let applied = s.call("apply_batch", args).await;
    assert_eq!(applied["committedRevision"], 1, "{applied}");
    // The compiler writes no height for `x`, and reads add none.
    let subtree = s
        .call("get_subtree", json!({"projectId":p,"nodeId":"x"}))
        .await;
    let props = &subtree["nodes"][0]["props"];
    assert_eq!(props["width"], 24, "{subtree}");
    assert!(props.get("height").is_none(), "{subtree}");

    let render = json!({"projectId":p,"revision":1,"frameId":"root",
        "viewport":{"width":320,"height":200}});
    let pending = s.call("render_frame", render).await;
    let job = s.wait(pending["jobId"].as_str().unwrap()).await;
    assert_eq!(job["status"], "ready", "{job}");
    let inspect = s
        .call(
            "inspect_layout",
            json!({"projectId":p,"jobId":job["jobId"],"sceneKey":job["sceneKey"],
                "nodeIds":["row","x","dot","tall"]}),
        )
        .await;
    assert_eq!(bounds(&inspect, "row")["height"], 55., "{inspect}");
    let x = bounds(&inspect, "x");
    assert_eq!(
        (&x["width"], &x["height"]),
        (&json!(24.), &json!(55.)),
        "{inspect}"
    );
    assert_eq!(bounds(&inspect, "dot")["height"], 8., "{inspect}");
}

/// The same frame as a component master, placed through an instance in the stretching row.
#[tokio::test]
async fn an_instance_of_a_free_master_without_h_stretches_to_its_row() {
    let s = Server::start().await;
    let p = s.project().await;
    let source = "page#p
  comp#icon w=24 free
    rect#dot x=4 y=4 8x8 fill=#111111
  frame#root w=320 col
    frame#row w=fill row gap=8 align=stretch
      use#x icon
      rect#tall 10x55 fill=#ef4444
";
    let args = json!({"projectId":p,"expectedRevision":0,"idempotencyKey":"fb27i","source":source});
    let applied = s.call("apply_batch", args).await;
    assert_eq!(applied["committedRevision"], 1, "{applied}");
    let render = json!({"projectId":p,"revision":1,"frameId":"root",
        "viewport":{"width":320,"height":200}});
    let pending = s.call("render_frame", render).await;
    let job = s.wait(pending["jobId"].as_str().unwrap()).await;
    assert_eq!(job["status"], "ready", "{job}");
    let inspect = s
        .call(
            "inspect_layout",
            json!({"projectId":p,"jobId":job["jobId"],"sceneKey":job["sceneKey"],
                "nodeIds":["row","x","tall"]}),
        )
        .await;
    let x = bounds(&inspect, "x");
    assert_eq!(
        (&x["width"], &x["height"]),
        (&json!(24.), &json!(55.)),
        "{inspect}"
    );
}
