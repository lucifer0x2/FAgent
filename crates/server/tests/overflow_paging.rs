//! FB-32: a job stores every overflowing text node; its record and context keep showing 12, and
//! `inspect_layout` pages the rest and finds any of them by `nodeIds`. Native backend: no worker.
#[allow(dead_code)]
mod native_support;
use native_support::Server;
use serde_json::{json, Value};

const TEXTS: usize = 20;

fn design() -> Value {
    let mut ops = vec![
        json!({"op":"create_node","id":"page","kind":"page","parentId":null,"props":{}}),
        json!({"op":"create_node","id":"frame","kind":"frame","parentId":"page","props":{"width":400,"height":600,"layout":{"mode":"free"}}}),
    ];
    for i in 0..TEXTS {
        ops.push(json!({"op":"create_node","id":format!("t{i}"),"kind":"text","parentId":"frame","props":{"text":"Overflowing label","textWrap":"nowrap","x":10,"y":i * 28,"width":8,"height":28,"fontId":"font_bundled","fontSize":20,"lineHeight":28}}));
    }
    json!(ops)
}

fn items(value: &Value) -> Vec<String> {
    value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["code"] == "TEXT_CONTENT_OUTSIDE_BOX")
        .map(|d| d["nodeId"].as_str().unwrap().to_owned())
        .collect()
}

fn summary(value: &Value) -> Value {
    value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "TEXT_OVERFLOW_SUMMARY")
        .unwrap()
        .clone()
}

#[tokio::test]
async fn every_overflowing_text_node_is_reachable_through_inspect_layout() {
    let s = Server::start().await;
    let p = s.project().await;
    assert_eq!(s.batch(&p, 0, design()).await["committedRevision"], 1);
    let pending = s
        .call(
            "render_frame",
            json!({"projectId":p,"revision":1,"frameId":"frame","viewport":{"width":400,"height":600}}),
        )
        .await;
    let job = s.wait(pending["jobId"].as_str().unwrap()).await;
    assert_eq!(job["status"], "ready", "{job}");
    // The record keeps its 12-item summary.
    assert_eq!(items(&job).len(), 12);
    let record = summary(&job);
    assert_eq!(
        (
            record["reportedNodes"].as_u64(),
            record["omittedNodes"].as_u64()
        ),
        (Some(12), Some(8))
    );

    let args =
        json!({"projectId":p,"jobId":job["jobId"],"sceneKey":job["sceneKey"],"fields":["bounds"]});
    let first = s.call("inspect_layout", args.clone()).await;
    assert_eq!(items(&first).len(), 12, "{first}");
    assert_eq!(first["diagnosticsPage"]["nextOffset"], 12);
    assert_eq!(first["diagnosticsPage"]["total"], TEXTS);
    let mut rest = args.clone();
    rest["diagnosticsOffset"] = json!(12);
    let rest = s.call("inspect_layout", rest).await;
    let mut all = items(&first);
    all.extend(items(&rest));
    let mut expected: Vec<_> = (0..TEXTS).map(|i| format!("t{i}")).collect();
    all.sort();
    expected.sort();
    assert_eq!(all, expected);
    assert_eq!(rest["diagnosticsPage"]["nextOffset"], Value::Null);
    let mut one = args.clone();
    one["diagnosticsLimit"] = json!(500);
    assert_eq!(items(&s.call("inspect_layout", one).await).len(), TEXTS);
    let mut late = args;
    late["nodeIds"] = json!(["t17"]);
    let late = s.call("inspect_layout", late).await;
    assert_eq!(items(&late), ["t17"], "{late}");
    assert!(late.get("diagnosticsPage").is_none());
}

/// A byte budget may drop items from a diagnostics page; `nextOffset` resumes after the last
/// item kept, so a client following it still sees every item.
#[tokio::test]
async fn budgeted_overflow_pages_skip_no_item() {
    let s = Server::start().await;
    let p = s.project().await;
    assert_eq!(s.batch(&p, 0, design()).await["committedRevision"], 1);
    let pending = s
        .call(
            "render_frame",
            json!({"projectId":p,"revision":1,"frameId":"frame","viewport":{"width":400,"height":600}}),
        )
        .await;
    let job = s.wait(pending["jobId"].as_str().unwrap()).await;
    let mut expected: Vec<_> = (0..TEXTS).map(|i| format!("t{i}")).collect();
    expected.sort();
    for limit in [12, 500] {
        let (mut seen, mut offset, mut calls) = (Vec::new(), 0, 0);
        loop {
            let args = json!({"projectId":p,"jobId":job["jobId"],"sceneKey":job["sceneKey"],
                "fields":["bounds"],"limit":1,"budget":8192,
                "diagnosticsLimit":limit,"diagnosticsOffset":offset});
            let page = s.call("inspect_layout", args).await;
            let shown = items(&page);
            assert!(!shown.is_empty(), "{page}");
            seen.extend(shown);
            calls += 1;
            assert!(calls <= TEXTS, "no progress: {page}");
            match page["diagnosticsPage"]["nextOffset"].as_u64() {
                Some(next) => offset = next,
                None => break,
            }
        }
        seen.sort();
        assert_eq!(seen, expected, "limit {limit}");
    }
}
