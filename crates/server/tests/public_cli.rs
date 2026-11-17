//! CLI validation and dual-listener shutdown with isolated roots and random ports.
use futures_util::StreamExt;
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

const BINARY: &str = env!("CARGO_BIN_EXE_fagent-server");
const ORIGIN: &str = "https://canvas.example.ts.net";

#[tokio::test]
async fn public_flags_fail_before_opening_the_data_root() {
    let root = tempfile::tempdir().unwrap();
    for args in [
        vec!["--public-viewer-port", "3848"],
        vec!["--public-viewer-origin", ORIGIN],
        vec![
            "--port",
            "3848",
            "--public-viewer-port",
            "3848",
            "--public-viewer-origin",
            ORIGIN,
        ],
        vec![
            "--public-viewer-port",
            "65536",
            "--public-viewer-origin",
            ORIGIN,
        ],
        vec![
            "--public-viewer-port",
            "0",
            "--public-viewer-origin",
            "http://canvas.example.ts.net",
        ],
    ] {
        let output = Command::new(BINARY)
            .arg("--data-root")
            .arg(root.path())
            .args(&args)
            .output()
            .await
            .unwrap();
        assert!(!output.status.success(), "{args:?}");
        assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
    }
}

#[tokio::test]
async fn invalid_additional_origins_fail_before_opening_the_data_root() {
    let root = tempfile::tempdir().unwrap();
    for origins in [
        "",
        ",",
        "https://*.site",
        "https://alternate.example.invalid/path",
        "https://alternate.example.invalid:65536",
    ] {
        let output = Command::new(BINARY)
            .env("FAGENT_PUBLIC_ALLOWED_ORIGINS", origins)
            .args([
                "--public-viewer-port",
                "0",
                "--public-viewer-origin",
                ORIGIN,
                "--data-root",
            ])
            .arg(root.path())
            .output()
            .await
            .unwrap();
        assert!(!output.status.success(), "{origins}");
        assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
    }
}

const LIVE: &str = "FAGENT_PUBLIC_LIVE_UPDATES";

#[tokio::test]
async fn invalid_live_update_switches_fail_before_opening_the_data_root() {
    let root = tempfile::tempdir().unwrap();
    let public = [
        "--public-viewer-port",
        "0",
        "--public-viewer-origin",
        ORIGIN,
    ];
    let cases: [(Option<&str>, Vec<&str>); 6] = [
        (Some("yes"), public.to_vec()),
        (Some("ON"), public.to_vec()),
        (Some(" on"), public.to_vec()),
        (
            None,
            [&public[..], &["--public-viewer-live-updates", "maybe"]].concat(),
        ),
        (
            None,
            [&public[..], &["--public-viewer-live-updates"]].concat(),
        ),
        (None, vec!["--public-viewer-live-updates", "on"]),
    ];
    for (env, args) in cases {
        let mut command = Command::new(BINARY);
        command.env_remove(LIVE);
        if let Some(value) = env {
            command.env(LIVE, value);
        }
        let output = command
            .arg("--data-root")
            .arg(root.path())
            .args(&args)
            .output()
            .await
            .unwrap();
        assert!(!output.status.success(), "{env:?} {args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.to_ascii_lowercase().contains("live"), "{stderr}");
        assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
    }
}

/// Startup prints the effective switch: off by default, the variable enables it, the flag wins.
#[tokio::test]
async fn live_updates_default_off_and_the_flag_overrides_the_environment() {
    for (env, flag, expected) in [
        (None, None, "off"),
        (Some(""), None, "off"),
        (Some("on"), None, "on"),
        (Some("1"), None, "on"),
        (Some("off"), Some("on"), "on"),
        (Some("on"), Some("off"), "off"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut command = Command::new(BINARY);
        command.env_remove(LIVE);
        if let Some(value) = env {
            command.env(LIVE, value);
        }
        if let Some(value) = flag {
            command.args(["--public-viewer-live-updates", value]);
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
        let public = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let line = lines.next_line().await.unwrap().unwrap();
                if line.starts_with("Public viewer ") {
                    break line;
                }
            }
        })
        .await
        .unwrap();
        assert!(
            public.ends_with(&format!("| live updates {expected}")),
            "{env:?} {flag:?}: {public}"
        );
        child.kill().await.unwrap();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn both_random_listeners_stop_with_active_sse_on_one_interrupt() {
    let root = tempfile::tempdir().unwrap();
    let mut child = Command::new(BINARY)
        .env(LIVE, "on")
        .env(
            "FAGENT_PUBLIC_ALLOWED_ORIGINS",
            "https://alternate.example.invalid",
        )
        .args([
            "--port",
            "0",
            "--public-viewer-port",
            "0",
            "--public-viewer-origin",
            ORIGIN,
            "--data-root",
        ])
        .arg(root.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let local = tokio::time::timeout(Duration::from_secs(10), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let public = lines.next_line().await.unwrap().unwrap();
    assert!(local.starts_with("FAgent http://127.0.0.1:"), "{local}");
    assert!(
        public.starts_with("Public viewer https://canvas.example.ts.net"),
        "{public}"
    );
    let local_url = local.split_whitespace().nth(1).unwrap();
    let public_addr = public
        .split("bound to ")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    let client = reqwest::Client::new();
    let secondary = client
        .get(format!("http://{public_addr}/api/v1/viewer"))
        .header("host", "alternate.example.invalid")
        .header("origin", "https://alternate.example.invalid")
        .send()
        .await
        .unwrap();
    assert_eq!(secondary.status(), 200);
    let local_stream = client
        .get(format!("{local_url}/api/v1/events"))
        .send()
        .await
        .unwrap();
    let public_stream = client
        .get(format!("http://{public_addr}/api/v1/events"))
        .header("host", "canvas.example.ts.net")
        .send()
        .await
        .unwrap();
    assert_eq!(local_stream.status(), 200);
    assert_eq!(public_stream.status(), 200);
    let mut local_stream = local_stream.bytes_stream();
    let mut public_stream = public_stream.bytes_stream();
    assert!(Command::new("kill")
        .args(["-INT", &child.id().unwrap().to_string()])
        .status()
        .await
        .unwrap()
        .success());
    // Less than the three-second hard-exit fallback: this must be graceful shutdown.
    let status = tokio::time::timeout(Duration::from_millis(2800), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(status.success());
    assert!(local_stream.next().await.is_none());
    assert!(public_stream.next().await.is_none());
    assert!(client
        .get(format!("{local_url}/api/v1/projects"))
        .send()
        .await
        .is_err());
    assert!(client
        .get(format!("http://{public_addr}/api/v1/projects"))
        .send()
        .await
        .is_err());
}
