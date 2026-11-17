//! Projects from `fixtures/generate.mjs`, written under the cargo test temp dir and
//! never printed. Needs Node.js >= 20 on PATH.
use fagent_scene_input::{validate::Candidate, Snapshot};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

/// Runs the generator; returns the snapshot and its stats line.
pub fn generate(nodes: usize, seed: u64) -> (Snapshot, Value) {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/generate.mjs");
    let out =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("fixture-{nodes}-{seed}.json"));
    let output = Command::new("node")
        .arg(&script)
        .args(["--nodes", &nodes.to_string(), "--seed", &seed.to_string()])
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap_or_else(|e| {
            panic!("`node` (Node.js >= 20) must be on PATH to generate fixtures: {e}")
        });
    assert!(
        output.status.success(),
        "generate.mjs failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stats: Value = serde_json::from_slice(&output.stdout).expect("generator stats");
    let snapshot =
        serde_json::from_slice(&std::fs::read(&out).expect("fixture file")).expect("snapshot JSON");
    (snapshot, stats)
}

/// Full scene-input validation; the panic names only the error.
pub fn validate_with_scene_input(snapshot: &Snapshot) {
    if let Err(e) = Candidate::from_snapshot(snapshot).validate() {
        panic!(
            "scene-input rejected the snapshot: {} {:?} {}",
            e.code, e.field, e.message
        );
    }
}
