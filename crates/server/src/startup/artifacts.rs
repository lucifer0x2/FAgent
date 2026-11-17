//! A `chromium` start needs the worker runtime next to the binary. A build made with
//! `FAGENT_RENDER_BACKEND=vello` does not package it, so fail at startup, not at first render.
use fagent_server::RenderBackend;
use std::{env, path::Path};

/// Files the Node worker loads from the workspace, and `layout-step` as the worker resolves it.
pub(super) fn check(backend: RenderBackend, workspace: &Path) -> Result<(), String> {
    if backend != RenderBackend::Chromium {
        return Ok(());
    }
    let layout = env::var_os("FAGENT_LAYOUT_BIN");
    let layout_path = layout
        .as_deref()
        .map(Path::new)
        .unwrap_or(Path::new("target/debug/layout-step"));
    let missing: Vec<&str> = [
        ("worker/cli.mjs", workspace.join("worker/cli.mjs")),
        (
            "renderer/dist/index.js",
            workspace.join("renderer/dist/index.js"),
        ),
        (
            if layout.is_some() {
                "layout-step (FAGENT_LAYOUT_BIN)"
            } else {
                "target/debug/layout-step"
            },
            workspace.join(layout_path),
        ),
    ]
    .into_iter()
    .filter(|(_, path)| !path.is_file())
    .map(|(name, _)| name)
    .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!(
        "RENDER_BACKEND=chromium needs the Chromium export runtime, but the workspace lacks {}. \
         This binary was built with FAGENT_RENDER_BACKEND={} (default render backend). \
         Start with RENDER_BACKEND=vello, or rebuild with FAGENT_RENDER_BACKEND=chromium \
         (npm run build) and point --workspace at that checkout.",
        missing.join(", "),
        RenderBackend::BUILD_DEFAULT.name()
    ))
}
