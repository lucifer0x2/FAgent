//! On PostgreSQL, `apply_batch_file` reads `.json` and `.ocd` files from the local
//! `<data-root>/<slug>/inbox`, as on SQLite; a rejected path names `inboxDir`.
#[allow(dead_code)] // Only `start_with_inbox` and `call` are used here.
mod postgres_backend_support;
use postgres_backend_support::Server;
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires fresh PostgreSQL via scripts/task19-check.sh"]
async fn postgres_applies_json_and_ocd_batch_files_from_the_inbox() {
    let root = tempfile::tempdir().unwrap();
    let server = Server::start_with_inbox(Some(root.path().into())).await;
    let slug = format!("batch-pg-{}", uuid::Uuid::new_v4().simple());
    let created = server
        .call("create_project", json!({"slug":slug,"name":"Batch"}))
        .await;
    let id = created["projectId"].as_str().unwrap().to_owned();
    let inbox = root.path().join(&slug).join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    let operations = json!([{"op":"create_node","id":"home","kind":"page","parentId":null,"props":{"name":"Home"}}]);
    std::fs::write(inbox.join("page.json"), operations.to_string()).unwrap();
    std::fs::write(
        inbox.join("frame.ocd"),
        "@in home\n  frame#hero 320x200 col\n",
    )
    .unwrap();
    let file = |revision: u64, key: &str, path: &str| json!({"projectId":id,"expectedRevision":revision,"idempotencyKey":key,"path":path});
    let page = server
        .call("apply_batch_file", file(0, "page", "page.json"))
        .await;
    assert_eq!(page["committedRevision"], 1, "{page}");
    let mut preview = file(1, "frame", "frame.ocd");
    preview["dryRun"] = json!(true);
    let dry = server.call("apply_batch_file", preview).await;
    assert_eq!(dry["dryRun"], true, "{dry}");
    let frame = server
        .call("apply_batch_file", file(1, "frame", "frame.ocd"))
        .await;
    assert_eq!(frame["committedRevision"], 2, "{frame}");
    let escape = server
        .call("apply_batch_file", file(2, "escape", "../page.json"))
        .await;
    assert_eq!(escape["error"]["code"], "PATH_REJECTED", "{escape}");
    let message = escape["error"]["message"].as_str().unwrap();
    assert!(message.contains("inboxDir from list_projects"), "{message}");
}
