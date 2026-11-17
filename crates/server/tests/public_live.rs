//! Public live updates: off by default, on by opt-in, never affecting the local listener.
#[allow(dead_code)]
mod public_support;
#[allow(dead_code)]
mod support;
use public_support::{set_privacy, PublicServer};
use reqwest::Method;
use serde_json::{json, Value};
use support::Server;

const PROJECTS: &str = "/api/v1/projects";

async fn status(public: &PublicServer, path: &str) -> (u16, Value) {
    let response = public.request(Method::GET, path).send().await.unwrap();
    let status = response.status().as_u16();
    assert_eq!(response.headers()["cache-control"], "no-store", "{path}");
    (status, response.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn default_public_listener_refuses_event_streams_and_serves_the_latest_snapshot() {
    let local = Server::start().await;
    let public = PublicServer::start(local.state.clone()).await;
    let shown = local.project().await;
    set_privacy(&local, &shown, "public").await;
    let hidden = local
        .call("create_project", json!({"slug":"hidden","name":"Hidden"}))
        .await["projectId"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        public.get("/api/v1/viewer").await,
        json!({"readOnly":true,"liveUpdates":false,"overscan":0.0})
    );
    for path in [
        "/api/v1/events".to_owned(),
        format!("{PROJECTS}/{shown}/events"),
        format!("{PROJECTS}/{shown}/events?cursor={shown}:0"),
    ] {
        let (code, body) = status(&public, &path).await;
        assert_eq!(code, 403, "{path}");
        assert_eq!(body["error"]["code"], "PUBLIC_LIVE_DISABLED", "{path}");
        assert_eq!(body["error"]["retryable"], false, "{path}");
    }
    // Project admission still runs first: a private or unknown project reads as missing.
    for key in [hidden.as_str(), "unknown"] {
        let (code, body) = status(&public, &format!("{PROJECTS}/{key}/events")).await;
        assert_eq!(code, 404, "{key}");
        assert_eq!(body["error"]["code"], "PROJECT_NOT_FOUND");
    }

    // Without a stream the page shows the revision current at load; a reload shows newer work.
    let snapshot = format!("{PROJECTS}/{shown}/snapshot");
    assert_eq!(public.get(&snapshot).await["revision"], 0);
    let page = json!([{"op":"create_node","id":"page","kind":"page","parentId":null,"props":{}}]);
    assert_eq!(
        local.batch(&shown, 0, "live-off", page).await["committedRevision"],
        1
    );
    assert_eq!(public.get(&snapshot).await["revision"], 1);
    let listed = public.get(PROJECTS).await;
    assert_eq!(listed["projects"][0]["revision"], 1, "{listed}");
}

#[tokio::test]
async fn local_listener_keeps_live_updates_while_the_public_one_has_none() {
    let local = Server::start().await;
    let _public = PublicServer::start(local.state.clone()).await;
    let project = local.project().await;
    set_privacy(&local, &project, "public").await;
    assert_eq!(
        local.get("/api/v1/viewer").await,
        json!({"readOnly":false,"liveUpdates":true,"overscan":0.0})
    );
    for path in [
        "/api/v1/events".to_owned(),
        format!("{PROJECTS}/{project}/events"),
    ] {
        let response = local
            .client
            .get(format!("{}{path}", local.url))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "{path}");
        assert_eq!(response.headers()["content-type"], "text/event-stream");
    }
}

#[tokio::test]
async fn enabled_public_live_updates_serve_both_streams() {
    let local = Server::start().await;
    let public = PublicServer::start_live(local.state.clone()).await;
    let project = local.project().await;
    set_privacy(&local, &project, "public").await;
    assert_eq!(
        public.get("/api/v1/viewer").await,
        json!({"readOnly":true,"liveUpdates":true,"overscan":0.0})
    );
    for path in [
        "/api/v1/events".to_owned(),
        format!("{PROJECTS}/{project}/events"),
    ] {
        let response = public.request(Method::GET, &path).send().await.unwrap();
        assert_eq!(response.status(), 200, "{path}");
        assert_eq!(response.headers()["content-type"], "text/event-stream");
    }
}
