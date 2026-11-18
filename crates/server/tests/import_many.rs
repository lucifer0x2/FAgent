//! `import_assets`: several inbox, inline or copied assets in one call commit as one
//! revision; any bad item fails the whole call and names `items[i]`; replay and the item cap
//! follow `import_asset`.
#[allow(dead_code)]
mod support;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
use support::Server;

const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aX1sAAAAASUVORK5CYII=";

/// Only the signature and minimum length are checked for WOFF2 and JPEG.
fn woff2() -> Vec<u8> {
    let mut bytes = b"wOF2".to_vec();
    bytes.resize(64, 0);
    bytes
}
fn jpeg() -> Vec<u8> {
    let mut bytes = b"\xff\xd8\xff".to_vec();
    bytes.resize(32, 1);
    bytes
}
fn inbox(s: &Server) -> PathBuf {
    s.root.path().join("test-project/inbox")
}
fn args(project: &str, revision: u64, key: &str, items: Value) -> Value {
    json!({"projectId":project,"expectedRevision":revision,"idempotencyKey":key,"items":items})
}
fn font() -> Value {
    json!({"id":"brand","family":"Brand","weight":400,"style":"normal"})
}
async fn error(s: &Server, args: Value) -> Value {
    let result = s.call("import_assets", args).await;
    assert!(result["error"].is_object(), "{result}");
    assert!(!result.to_string().contains(s.root.path().to_str().unwrap()));
    result["error"].clone()
}

#[tokio::test]
async fn several_assets_commit_as_one_revision_in_order_and_replay() {
    let s = Server::start().await;
    let p = s.project().await;
    fs::write(inbox(&s).join("logo.png"), STANDARD.decode(PNG).unwrap()).unwrap();
    fs::create_dir(inbox(&s).join("fonts")).unwrap();
    fs::write(inbox(&s).join("fonts/brand.woff2"), woff2()).unwrap();
    let items = json!([
        {"path":"logo.png","mediaType":"image/png"},
        {"path":"fonts/brand.woff2","mediaType":"font/woff2","font":font()},
        {"base64":STANDARD.encode(jpeg()),"mediaType":"image/jpeg"}
    ]);
    let first = s
        .call("import_assets", args(&p, 0, "assets", items.clone()))
        .await;
    assert_eq!(first["committedRevision"], 1, "{first}");
    assert_eq!(first["replayed"], false);
    let listed = first["items"].as_array().unwrap();
    let media: Vec<&str> = listed
        .iter()
        .map(|i| i["asset"]["mediaType"].as_str().unwrap())
        .collect();
    assert_eq!(media, ["image/png", "font/woff2", "image/jpeg"]);
    assert_eq!(listed[1]["font"]["id"], "brand");
    assert!(listed[0].get("font").is_none());
    let changed = first["changedIds"].as_array().unwrap();
    assert_eq!(changed.len(), 4, "three assets and the font: {first}");
    // One commit for every asset.
    let changes = s
        .call("get_changes", json!({"projectId":p,"sinceRevision":0}))
        .await;
    assert_eq!(changes["commits"].as_array().unwrap().len(), 1, "{changes}");
    let fonts = s.call("list_fonts", json!({"projectId":p})).await;
    assert!(fonts.to_string().contains("\"brand\""), "{fonts}");
    // Same key and content (base64 for the inbox file this time): the stored result.
    let mut same = items.clone();
    same[0] = json!({"base64":PNG,"mediaType":"image/png"});
    let replay = s.call("import_assets", args(&p, 0, "assets", same)).await;
    assert_eq!(replay["replayed"], true, "{replay}");
    assert_eq!(replay["items"], first["items"]);
    // Changed content under the used key is rejected.
    fs::write(inbox(&s).join("logo.png"), jpeg()).unwrap();
    let mut changed = items;
    changed[0]["mediaType"] = json!("image/jpeg");
    let reused = error(&s, args(&p, 0, "assets", changed)).await;
    assert_eq!(reused["code"], "IDEMPOTENCY_KEY_REUSED", "{reused}");
}

#[tokio::test]
async fn one_bad_item_fails_the_whole_call_and_names_it() {
    let s = Server::start().await;
    let p = s.project().await;
    fs::write(inbox(&s).join("logo.png"), STANDARD.decode(PNG).unwrap()).unwrap();
    fs::write(
        s.root.path().join("outside.png"),
        STANDARD.decode(PNG).unwrap(),
    )
    .unwrap();
    let good = json!({"path":"logo.png","mediaType":"image/png"});
    let cases = [
        (
            json!({"path":"missing.png","mediaType":"image/png"}),
            "FILE_NOT_FOUND",
            "items[1].path",
        ),
        (
            json!({"path":"../outside.png","mediaType":"image/png"}),
            "PATH_REJECTED",
            "items[1].path",
        ),
        (
            json!({"path":"logo.png","base64":PNG,"mediaType":"image/png"}),
            "INVALID_REQUEST",
            "items[1].base64",
        ),
        (
            json!({"sourceProjectId":"nope","assetId":"a","mediaType":"image/png"}),
            "INVALID_REQUEST",
            "items[1].mediaType",
        ),
        (
            json!({"path":"logo.png","mediaType":"image/webp"}),
            "VALIDATION_FAILED",
            "items[1].mediaType",
        ),
        (
            json!({"base64":"!!!!","mediaType":"image/png"}),
            "INVALID_REQUEST",
            "items[1].source",
        ),
    ];
    for (bad, code, field) in cases {
        let failed = error(&s, args(&p, 0, "bad", json!([good.clone(), bad]))).await;
        assert_eq!(failed["code"], code, "{failed}");
        assert_eq!(failed["field"], field, "{failed}");
        let message = failed["message"].as_str().unwrap();
        assert!(message.starts_with("items[1]"), "{failed}");
    }
    // The same font ID from two items.
    fs::write(inbox(&s).join("brand.woff2"), woff2()).unwrap();
    let face = json!({"path":"brand.woff2","mediaType":"font/woff2","font":font()});
    let twice = error(&s, args(&p, 0, "fonts", json!([face.clone(), face]))).await;
    assert_eq!(twice["field"], "items[1].font", "{twice}");
    // A stale revision fails the whole call too.
    let stale = error(&s, args(&p, 4, "stale", json!([good]))).await;
    assert_eq!(stale["code"], "REVISION_CONFLICT", "{stale}");
    let snapshot = s.get(&format!("/api/v1/projects/{p}/snapshot")).await;
    assert_eq!(snapshot["revision"], 0);
    assert_eq!(snapshot["assets"], json!([]));
}

#[tokio::test]
async fn item_count_is_capped_at_fifty() {
    let s = Server::start().await;
    let p = s.project().await;
    let item = json!({"base64":PNG,"mediaType":"image/png"});
    let too_many = Value::Array(vec![item.clone(); 51]);
    let capped = error(&s, args(&p, 0, "many", too_many)).await;
    assert_eq!(capped["code"], "INVALID_REQUEST", "{capped}");
    let none = error(&s, args(&p, 0, "none", json!([]))).await;
    assert_eq!(none["code"], "INVALID_REQUEST", "{none}");
    // Fifty identical items are one asset in one revision.
    let fifty = Value::Array(vec![item; 50]);
    let result = s.call("import_assets", args(&p, 0, "fifty", fifty)).await;
    assert_eq!(result["committedRevision"], 1, "{result}");
    assert_eq!(result["items"].as_array().unwrap().len(), 50);
    assert_eq!(result["changedIds"].as_array().unwrap().len(), 1);
}
