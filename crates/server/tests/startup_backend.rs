use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    time::Duration,
};

const SECRET: &str = "backend-secret-must-not-appear";

fn server() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fagent-server"));
    for name in [
        "RENDER_BACKEND",
        "DATABASE_PROVIDER",
        "DATABASE_URL",
        "PORT",
        "FAGENT_DATA_ROOT",
        "HOME",
    ] {
        command.env_remove(name);
    }
    command
}

fn failure(output: Output, message: &str) {
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains(message));
    for stream in [&output.stdout, &output.stderr] {
        assert!(!String::from_utf8_lossy(stream).contains(SECRET));
    }
}

#[test]
fn render_backend_env_requires_an_exact_supported_value() {
    for value in [
        "",
        "rust",
        "Rust",
        "RUST",
        "rust ",
        " chromium",
        "Vello",
        "vello-gpu",
        SECRET,
    ] {
        failure(
            server().env("RENDER_BACKEND", value).output().unwrap(),
            "RENDER_BACKEND must be chromium or vello",
        );
    }
    for value in ["chromium", "vello"] {
        failure(
            server().env("RENDER_BACKEND", value).output().unwrap(),
            "Data root is required",
        );
    }
}

#[cfg(unix)]
#[test]
fn non_unicode_backend_env_is_rejected_without_echoing_it() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    failure(
        server()
            .env("RENDER_BACKEND", OsString::from_vec(vec![0xff]))
            .output()
            .unwrap(),
        "RENDER_BACKEND must be chromium or vello",
    );
}

#[test]
fn cli_backend_is_validated_and_overrides_env() {
    for value in ["vello", "chromium"] {
        failure(
            server()
                .env("RENDER_BACKEND", SECRET)
                .args(["--render-backend", value])
                .output()
                .unwrap(),
            "Data root is required",
        );
    }
    failure(
        server().arg("--render-backend").output().unwrap(),
        "--render-backend needs chromium or vello",
    );
    failure(
        server()
            .args(["--render-backend", SECRET])
            .output()
            .unwrap(),
        "--render-backend must be chromium or vello",
    );
}

#[test]
fn help_ignores_invalid_backend_env_even_after_other_options() {
    let output = server()
        .env("RENDER_BACKEND", SECRET)
        .env("DATABASE_PROVIDER", SECRET)
        .env("PORT", SECRET)
        .args(["--render-backend", SECRET, "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("RENDER_BACKEND") && help.contains("--render-backend"));
    assert!(!help.contains(SECRET));
}

struct RunningServer(Child);

impl Drop for RunningServer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn native_startup_with_empty_path_uses_only_temporary_data_root() {
    let backend = "vello";
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data");
    let mut process = RunningServer(
        server()
            .env("PATH", "")
            .env("RENDER_BACKEND", backend)
            .arg("--data-root")
            .arg(&root)
            .args(["--port", "0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stdout = process.0.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = sender.send(result);
    });
    let ready = receiver.recv_timeout(Duration::from_secs(20));
    // Stop even on timeout; startup success is not evidence of native render dispatch.
    drop(process);
    reader.join().unwrap();
    let ready = ready.expect("server startup timed out").unwrap();
    let address = ready
        .strip_prefix("FAgent http://127.0.0.1:")
        .expect("server did not announce readiness");
    let port: u16 = address.split(' ').next().unwrap().parse().unwrap();
    assert_ne!(port, 0);
    assert!(root.is_dir());
    assert!(!root.join("db.sql").exists());
}

#[test]
fn chromium_without_its_worker_runtime_fails_at_startup_naming_the_build_variable() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("vello-release");
    std::fs::create_dir(&workspace).unwrap();
    let root = temp.path().join("data");
    let output = server()
        .env("RENDER_BACKEND", "chromium")
        .env_remove("FAGENT_LAYOUT_BIN")
        .arg("--workspace")
        .arg(&workspace)
        .arg("--data-root")
        .arg(&root)
        .args(["--port", "0"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    for expected in [
        "RENDER_BACKEND=chromium",
        "FAGENT_RENDER_BACKEND",
        "worker/cli.mjs",
        "renderer/dist/index.js",
        "layout-step",
    ] {
        assert!(stderr.contains(expected), "{expected}: {stderr}");
    }
    assert!(!root.exists(), "the data root must not be created");
}
