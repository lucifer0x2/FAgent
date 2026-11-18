//! Published tool input schemas must be plain top-level objects: MCP requires `"type":"object"`
//! and the Anthropic API rejects top-level combinators, so one bad schema fails the whole
//! `tools/list` for those clients. `render_frame` still enforces its two exclusive modes at
//! runtime. No browser is needed: accepted calls stop at a later, non-schema error.
#[allow(dead_code)] // `root`, `state`, `batch`, `get`, `wait_job` and `design` are unused here.
mod support;
mod tool_names;
use serde_json::{json, Value};
use support::Server;

const INVALID: &str = "Arguments do not match the published tool schema";

#[tokio::test]
async fn every_published_input_schema_is_a_plain_object() {
    let s = Server::start().await;
    let tools = s.rpc("tools/list", json!({})).await;
    let tools = tools["tools"].as_array().unwrap();
    tool_names::assert_listed(tools);
    for tool in tools {
        let schema = tool["inputSchema"].as_object().unwrap();
        let name = &tool["name"];
        assert_eq!(schema.get("type"), Some(&json!("object")), "{name}");
        assert!(
            schema.get("properties").is_some_and(Value::is_object),
            "{name}"
        );
        for combinator in ["oneOf", "anyOf", "allOf", "not"] {
            assert!(!schema.contains_key(combinator), "{name}: {combinator}");
        }
    }
    let render = tools.iter().find(|t| t["name"] == "render_frame").unwrap();
    let schema = &render["inputSchema"];
    let mut fields: Vec<_> = schema["properties"].as_object().unwrap().keys().collect();
    fields.sort();
    let expected = [
        "frameId",
        "includeScene",
        "jobId",
        "projectId",
        "revision",
        "scale",
        "state",
        "theme",
        "viewport",
        "waitMs",
    ];
    assert_eq!(fields, expected);
    assert_eq!(schema["required"], json!(["projectId"]));
    assert_eq!(schema["additionalProperties"], false);
}

async fn code(s: &Server, args: Value) -> Value {
    let result = s.call("render_frame", args.clone()).await;
    if result["error"]["code"] == "INVALID_REQUEST" {
        let message = result["error"]["message"].as_str().unwrap();
        assert!(message.starts_with(INVALID), "{args}: {result}");
    }
    result["error"]["code"].clone()
}

#[tokio::test]
async fn render_frame_modes_are_still_exclusive_at_runtime() {
    let s = Server::start().await;
    let p = s.project().await;
    let viewport = json!({"width":320,"height":200});
    // Accepted shapes pass the schema and stop at the job or revision lookup.
    let enqueue = json!({"projectId":p,"revision":7,"frameId":"frame","viewport":viewport});
    assert_eq!(code(&s, enqueue.clone()).await, "REVISION_UNAVAILABLE");
    let full = json!({"projectId":p,"revision":7,"frameId":"frame","viewport":viewport,"theme":"night","state":"default","scale":2});
    assert_eq!(code(&s, full).await, "REVISION_UNAVAILABLE");
    let poll = json!({"projectId":p,"jobId":"job_missing"});
    assert_eq!(code(&s, poll.clone()).await, "JOB_NOT_FOUND");
    let wait = json!({"projectId":p,"jobId":"job_missing","waitMs":100});
    assert_eq!(code(&s, wait).await, "JOB_NOT_FOUND");
    // Mixed modes, waitMs without jobId, neither mode, unknown fields and bad values.
    let mut rejected = vec![
        json!({"projectId":p,"jobId":"job_missing","frameId":"frame"}),
        json!({"projectId":p,"jobId":"job_missing","revision":7}),
        json!({"projectId":p,"jobId":"job_missing","viewport":viewport}),
        json!({"projectId":p,"jobId":"job_missing","theme":"night"}),
        json!({"projectId":p,"waitMs":100}),
        json!({"projectId":p}),
        json!({}),
        json!({"jobId":"job_missing"}),
        json!({"projectId":p,"jobId":"job_missing","extra":1}),
        json!({"projectId":p,"jobId":"job_missing","waitMs":10_001}),
        json!({"projectId":p,"revision":7,"frameId":"frame"}),
    ];
    for extra in [
        json!({"jobId":"job_missing"}),
        json!({"waitMs":100}),
        json!({"extra":1}),
        json!({"scale":5}),
        json!({"state":"unknown"}),
    ] {
        let mut args = enqueue.clone();
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        rejected.push(args);
    }
    for args in rejected {
        assert_eq!(code(&s, args.clone()).await, "INVALID_REQUEST", "{args}");
    }
}

/// A rejection names the failing field (message, `field`, and `opIndex` for an operation),
/// including the operations read from an `apply_batch_file` JSON file.
#[tokio::test]
async fn schema_rejections_name_the_failing_field() {
    let s = Server::start().await;
    let p = s.project().await;
    let inbox = s.root.path().join("test-project/inbox");
    let ops = r#"[{"op":"create_node","id":"pg","kind":"page","parentId":null,"props":{}},
        {"op":"create_node","id":"f","kind":"fram","parentId":"pg","props":{}}]"#;
    std::fs::write(inbox.join("ops.json"), ops).unwrap();
    std::fs::write(inbox.join("op.json"), r#"[{"op":"create","id":"x"}]"#).unwrap();
    let file = |key: &str, path: &str| json!({"projectId":p,"idempotencyKey":key,"path":path});
    let cases = [
        (
            "apply_batch_file",
            file("k", "ops.json"),
            "operations[1].kind",
            Some(1),
        ),
        (
            "apply_batch_file",
            file("k", "op.json"),
            "operations[0].op",
            Some(0),
        ),
        (
            "apply_batch_file",
            file("k.1", "ops.json"),
            "idempotencyKey",
            None,
        ),
        (
            "apply_batch_file",
            json!({"projectId":p,"idempotencyKey":"k"}),
            "path",
            None,
        ),
        (
            "apply_batch",
            json!({"projectId":p,"idempotencyKey":"k","operations":[{"op":"delete_subtree","id":"x","extra":1}]}),
            "operations[0].extra",
            Some(0),
        ),
        (
            "import_asset",
            json!({"projectId":p,"expectedRevision":0,"idempotencyKey":"k","path":"a.png","base64":"AAAA","mediaType":"image/png"}),
            "base64",
            None,
        ),
        (
            "render_frame",
            json!({"projectId":p,"jobId":"j","waitMs":10_001}),
            "waitMs",
            None,
        ),
        // The variant that accepts every field sent is the one blamed.
        (
            "apply_batch",
            json!({"projectId":p,"idempotencyKey":"k.1","source":"page#a \"A\""}),
            "idempotencyKey",
            None,
        ),
        (
            "import_asset",
            json!({"projectId":p,"expectedRevision":0,"idempotencyKey":"k","base64":"AAAA"}),
            "mediaType",
            None,
        ),
        (
            "import_asset",
            json!({"projectId":p,"expectedRevision":0,"idempotencyKey":"k.1","sourceProjectId":"s","assetId":"a"}),
            "idempotencyKey",
            None,
        ),
        // A missing discriminator, and a value of the right type that breaks a limit.
        (
            "apply_batch",
            json!({"projectId":p,"idempotencyKey":"k","operations":[{"id":"x"}]}),
            "operations[0].op",
            Some(0),
        ),
        (
            "apply_batch",
            json!({"projectId":p,"idempotencyKey":"k","operations":[{"op":"upsert_token","token":{"id":"t","name":"T","type":"color","value":"#0123456789abcdef0123"}}]}),
            "operations[0].token.value",
            Some(0),
        ),
    ];
    for (tool, args, field, op_index) in cases {
        let error = &s.call(tool, args.clone()).await["error"];
        assert_eq!(error["code"], "INVALID_REQUEST", "{tool} {args}: {error}");
        assert_eq!(error["field"], field, "{tool} {args}: {error}");
        let message = error["message"].as_str().unwrap();
        assert!(
            message.starts_with(&format!("{INVALID} at {field}: ")),
            "{message}"
        );
        assert_eq!(error["opIndex"], json!(op_index), "{tool} {args}: {error}");
    }
    let message = |error: &Value| error["error"]["message"].as_str().unwrap().to_owned();
    // Tied variants name what each needs; a missing op lists the accepted ones.
    let neither = s
        .call("apply_batch", json!({"projectId":p,"idempotencyKey":"k"}))
        .await;
    assert!(neither["error"].get("field").is_none(), "{neither}");
    assert!(message(&neither).ends_with(": is missing fields; send one of operations, source"));
    let no_op = json!({"projectId":p,"idempotencyKey":"k","operations":[{"id":"x"}]});
    let no_op = s.call("apply_batch", no_op).await;
    assert!(
        message(&no_op).contains("op: is required: one of \"create_node\""),
        "{no_op}"
    );
    let token = json!({"projectId":p,"idempotencyKey":"k","operations":[{"op":"upsert_token","token":{"id":"t","name":"T","type":"color","value":"#0123456789abcdef0123"}}]});
    let token = s.call("apply_batch", token).await;
    assert!(
        message(&token).ends_with("must have at most 16 characters"),
        "{token}"
    );
    // Caller-chosen key names are echoed at most 64 characters long.
    let long = "x".repeat(5000);
    let unknown = s.call("list_projects", json!({ long.clone(): 1 })).await;
    let field = unknown["error"]["field"].as_str().unwrap();
    assert_eq!(field, format!("{}…", &long[..64]));
    assert!(message(&unknown).len() < 200, "{unknown}");
    // Keys that are empty or could read as several steps are quoted.
    for (key, quoted) in [("", r#"[""]"#), ("a.b", r#"["a.b"]"#)] {
        let error = s.call("list_projects", json!({ key: 1 })).await;
        assert_eq!(error["error"]["field"], quoted, "{error}");
    }
    // A field every form needs is named; two sources sent together are both named.
    let no_project = s.call("apply_batch", json!({"idempotencyKey":"k"})).await;
    assert_eq!(no_project["error"]["field"], "projectId", "{no_project}");
    assert!(message(&no_project).contains("send one of"), "{no_project}");
    let both = json!({"projectId":p,"expectedRevision":0,"idempotencyKey":"k","path":"a.png","base64":"AAAA","mediaType":"image/png"});
    let both = s.call("import_asset", both).await;
    assert!(
        message(&both).ends_with("send only one of base64, path"),
        "{both}"
    );
    // An object value is blamed on the object form that accepts its known fields.
    let alias = json!({"projectId":p,"idempotencyKey":"k","operations":[{"op":"upsert_token","token":{"id":"t","name":"T","type":"color","value":{"aliasId":"a","extra":true}}}]});
    let alias = s.call("apply_batch", alias).await;
    assert_eq!(
        alias["error"]["field"], "operations[0].token.value.extra",
        "{alias}"
    );
    // import_assets names the same full path in the message and `field`.
    let items = json!({"projectId":p,"expectedRevision":0,"idempotencyKey":"k","items":[{"sourceProjectId":"s","assetId":"a","mediaType":"image/png"}]});
    let items = s.call("import_assets", items).await;
    assert_eq!(items["error"]["field"], "items[0].mediaType", "{items}");
    assert!(
        message(&items).contains(" at items[0].mediaType: "),
        "{items}"
    );
}
