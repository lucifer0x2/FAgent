mod storage_backend_support;
use serde_json::json;
use std::sync::Arc;
use storage_backend_support::{
    memory::{MemoryBackend, ASSET_BYTES, FONT_BYTES},
    Server,
};

#[tokio::test]
async fn independent_backend_serves_http_mcp_mutation_history_and_resource_bytes() {
    let backend = Arc::new(MemoryBackend::default());
    let server = Server::start(backend.clone()).await;
    assert_eq!(server.get("/api/v1/projects").await["projects"], json!([]));
    let created = server
        .client
        .post(format!("{}/api/v1/projects", server.url))
        .json(&json!({"slug":"in-memory","name":"Independent"}))
        .send()
        .await
        .unwrap();
    assert!(created.status().is_success());
    let created: serde_json::Value = created.json().await.unwrap();
    let id = created["projectId"].as_str().unwrap();
    assert!(id.starts_with("memory_"), "{created}");
    let listed = server.call("list_projects", json!({})).await;
    assert_eq!(listed["projects"][0]["projectId"], id);
    let args = json!({"projectId":id,"expectedRevision":0,"idempotencyKey":"one", "operations":[{"op":"create_node","id":"page","kind":"page","parentId":null,"props":{"name":"Memory only"}}]});
    let applied = server.call("apply_batch", args.clone()).await;
    assert_eq!(applied["committedRevision"], 1, "{applied}");
    assert_eq!(applied["replayed"], false);
    let replay = server.call("apply_batch", args).await;
    assert_eq!(replay["replayed"], true, "{replay}");
    assert_eq!(replay["committedRevision"], 1);
    let snapshot = server.get("/api/v1/projects/in-memory/snapshot").await;
    assert_eq!(snapshot["projectId"], id);
    assert_eq!(snapshot["revision"], 1);
    assert_eq!(snapshot["nodes"][0]["props"]["name"], "Memory only");
    let storage = fagent_storage::Storage::from_shared(backend.clone());
    let first = storage.shared_database(id).unwrap();
    let second = storage.shared_database(id).unwrap();
    assert!(
        !Arc::ptr_eq(&first.indexes, &second.indexes),
        "custom backend without identity must not cache indexes"
    );
    let guard = server
        .call(
            "get_edit_context",
            json!({"projectId":id,"pageIds":["page"]}),
        )
        .await;
    assert_eq!(guard["expectedRevision"], 1, "{guard}");
    let changes = server
        .call(
            "get_changes",
            json!({"projectId":id,"sinceRevision":0,"includeOperations":true}),
        )
        .await;
    assert_eq!(changes["currentRevision"], 1, "{changes}");
    assert_eq!(changes["commits"].as_array().unwrap().len(), 1);
    assert_eq!(changes["changed"]["nodes"], json!(["page"]));
    let asset = server
        .client
        .get(format!(
            "{}/api/v1/projects/{id}/assets/memory_asset",
            server.url
        ))
        .send()
        .await
        .unwrap();
    assert!(asset.status().is_success());
    assert_eq!(asset.bytes().await.unwrap().as_ref(), ASSET_BYTES);
    let fonts = server.get(&format!("/api/v1/projects/{id}/fonts")).await;
    assert_eq!(fonts["fonts"][0]["family"], "Memory Fixture");
    let font = server
        .client
        .get(format!(
            "{}/api/v1/projects/{id}/fonts/memory_font/download",
            server.url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(font.headers()["content-type"], "font/ttf");
    assert_eq!(font.bytes().await.unwrap().as_ref(), FONT_BYTES);
    let unsupported = server.call("apply_batch", json!({"projectId":id,"expectedRevision":1,"idempotencyKey":"preview","dryRun":true,"operations":[{"op":"delete_subtree","id":"page"}]})).await;
    assert_eq!(unsupported["error"]["code"], "FIXTURE_UNSUPPORTED");
    let calls = backend.calls();
    for expected in [
        "list", "create", "meta", "snapshot", "apply", "commits", "asset", "font", "dry-run",
    ] {
        assert!(
            calls.contains(&expected),
            "missing {expected} from {calls:?}"
        );
    }
    // The fixture has no disk root and AppState must not initialize a fallback project.
    assert_eq!(std::fs::read_dir(server.root.path()).unwrap().count(), 0);
}

/// A backend without staged batches (as PostgreSQL) reads `apply_batch_file` from the
/// configured inbox root: `.ocd` source compiles like inline `source`, JSON applies as
/// operations, and without a root the explicit unsupported error stays.
#[tokio::test]
async fn backend_without_staging_reads_batch_files_from_the_inbox_root() {
    let bare = Server::start(Arc::new(MemoryBackend::default())).await;
    let created = bare
        .call("create_project", json!({"slug":"bare","name":"Bare"}))
        .await;
    let id = created["projectId"].as_str().unwrap();
    let args = json!({"projectId":id,"expectedRevision":0,"idempotencyKey":"f","path":"page.ocd"});
    let error = bare.call("apply_batch_file", args).await;
    assert_eq!(error["error"]["code"], "STAGING_UNSUPPORTED", "{error}");
    // Without an inbox root there is no inboxDir to report.
    assert!(created.get("inboxDir").is_none(), "{created}");
    let listed = bare.call("list_projects", json!({})).await;
    assert!(listed["projects"][0].get("inboxDir").is_none(), "{listed}");

    let inbox_root = tempfile::tempdir().unwrap();
    let storage = fagent_storage::Storage::from_shared(Arc::new(MemoryBackend::default()))
        .with_inbox_root(inbox_root.path());
    let server = Server::with_storage(storage).await;
    let created = server
        .call("create_project", json!({"slug":"staged","name":"Staged"}))
        .await;
    let id = created["projectId"].as_str().unwrap();
    // MCP reports the absolute, canonical inbox; the server creates nothing there.
    let inbox = inbox_root
        .path()
        .canonicalize()
        .unwrap()
        .join("staged/inbox");
    assert_eq!(created["inboxDir"], inbox.to_str().unwrap(), "{created}");
    assert!(!inbox.exists());
    let listed = server.call("list_projects", json!({})).await;
    assert_eq!(listed["projects"][0]["inboxDir"], created["inboxDir"]);
    // The HTTP API (shared with the public viewer listener) never carries it.
    let http = server.get("/api/v1/projects").await;
    assert_eq!(http["projects"][0]["projectId"], id);
    assert!(http["projects"][0].get("inboxDir").is_none(), "{http}");
    let one = server.get("/api/v1/projects/staged").await;
    assert!(one.get("inboxDir").is_none(), "{one}");
    std::fs::create_dir_all(&inbox).unwrap();
    let source = "page#home \"Home\"\n";
    std::fs::write(inbox.join("page.ocd"), format!("\u{feff}{source}")).unwrap();
    let args =
        json!({"projectId":id,"expectedRevision":0,"idempotencyKey":"ocd","path":"page.ocd"});
    let committed = server.call("apply_batch_file", args).await;
    assert_eq!(committed["committedRevision"], 1, "{committed}");
    assert_eq!(committed["changedIds"], json!(["home"]));
    // Replay hashes the compiled operations: inline source under the same key replays.
    let inline =
        json!({"projectId":id,"expectedRevision":0,"idempotencyKey":"ocd","source":source});
    let replay = server.call("apply_batch", inline).await;
    assert_eq!(replay["replayed"], true, "{replay}");
    let ops = json!([{"op":"create_node","id":"second","kind":"page","parentId":null,"props":{"name":"JSON"}}]);
    std::fs::write(inbox.join("ops.json"), serde_json::to_vec(&ops).unwrap()).unwrap();
    // An absolute path under the reported inboxDir is accepted.
    let absolute = format!("{}/ops.json", created["inboxDir"].as_str().unwrap());
    let args = json!({"projectId":id,"expectedRevision":1,"idempotencyKey":"json","path":absolute});
    let committed = server.call("apply_batch_file", args).await;
    assert_eq!(committed["committedRevision"], 2, "{committed}");
    let args =
        json!({"projectId":id,"expectedRevision":2,"idempotencyKey":"x","path":"../ops.json"});
    let error = server.call("apply_batch_file", args).await;
    assert_eq!(error["error"]["code"], "PATH_REJECTED", "{error}");
}
