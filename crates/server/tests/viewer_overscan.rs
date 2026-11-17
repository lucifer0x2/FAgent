//! FAGENT_VIEWER_OVERSCAN: validated at startup and reported by `/api/v1/viewer` on both
//! listeners as a ratio. Isolated data roots and random loopback ports.
use serde_json::Value;
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

const BINARY: &str = env!("CARGO_BIN_EXE_fagent-server");
const ORIGIN: &str = "https://canvas.example.ts.net";
const OVERSCAN: &str = "FAGENT_VIEWER_OVERSCAN";

#[tokio::test]
async fn invalid_overscan_fails_naming_the_variable_before_opening_the_data_root() {
    let root = tempfile::tempdir().unwrap();
    for value in ["51", "-1", "1e1", "10%", " 10", "ten", ".5"] {
        let output = Command::new(BINARY)
            .env(OVERSCAN, value)
            .arg("--data-root")
            .arg(root.path())
            .args(["--port", "0"])
            .output()
            .await
            .unwrap();
        assert!(!output.status.success(), "{value}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(OVERSCAN), "{value}: {stderr}");
        assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
    }
}

/// `(local, public)` `/api/v1/viewer` bodies of a server started with `value` (None: unset).
async fn capabilities(value: Option<&str>) -> (Value, Value) {
    let root = tempfile::tempdir().unwrap();
    let mut command = Command::new(BINARY);
    command.env_remove(OVERSCAN);
    if let Some(value) = value {
        command.env(OVERSCAN, value);
    }
    let mut child = command
        .args(["--port", "0", "--public-viewer-port", "0"])
        .args(["--public-viewer-origin", ORIGIN, "--data-root"])
        .arg(root.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let (local, public) = tokio::time::timeout(Duration::from_secs(10), async {
        let local = lines.next_line().await.unwrap().unwrap();
        let public = lines.next_line().await.unwrap().unwrap();
        (local, public)
    })
    .await
    .unwrap();
    let local_url = local.split_whitespace().nth(1).unwrap().to_owned();
    let public_addr = public
        .split("bound to ")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned();
    let client = reqwest::Client::new();
    let get = |request: reqwest::RequestBuilder| async move {
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), 200);
        response.json::<Value>().await.unwrap()
    };
    let local = get(client.get(format!("{local_url}/api/v1/viewer"))).await;
    let public = get(client
        .get(format!("http://{public_addr}/api/v1/viewer"))
        .header("host", "canvas.example.ts.net"))
    .await;
    child.kill().await.unwrap();
    (local, public)
}

#[tokio::test]
async fn overscan_defaults_to_zero_and_both_listeners_report_the_ratio() {
    for (value, ratio) in [
        (None, 0.0),
        (Some(""), 0.0),
        (Some("0"), 0.0),
        (Some("10"), 0.1),
        (Some("12.5"), 0.125),
        (Some("50"), 0.5),
    ] {
        let (local, public) = capabilities(value).await;
        assert_eq!(
            local["overscan"].as_f64(),
            Some(ratio),
            "{value:?}: {local}"
        );
        assert_eq!(
            public["overscan"].as_f64(),
            Some(ratio),
            "{value:?}: {public}"
        );
        assert_eq!(local["readOnly"], false);
        assert_eq!(public["readOnly"], true);
    }
}
