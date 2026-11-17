# Task 29 — Vello GPU spike (P0)

The `scene-3` scene is drawn by one shared encoder (`crates/paint-vello`) for `vello_cpu` (native PNG, parity reference)
and `vello_gpu` (WebGPU primary, WebGL2 fallback) in the browser. Results and recommendation:
[docs/VELLO-GPU-RENDERER-RESEARCH.md § P0 results](../../docs/VELLO-GPU-RENDERER-RESEARCH.md#p0-results).

All output (`out/`, `pkg/`) is ignored; do not commit measurement PNG/JSON.

## Components

| File | Role |
| --- | --- |
| `wasm/` | crate `fagent-vello-gpu-demo` (cdylib wasm32): `Demo.webgpu(canvas)` / `Demo.webgl2(canvas)`, `set_scene`, `add_font`, `add_image`, `set_text_mode`, `set_greek_below`, `render`, `gpu_idle`. |
| `build.mjs` | Builds release wasm32 + `wasm-bindgen --target web` → `pkg/<profile>/`, writes `pkg/sizes.json` (raw/gzip/brotli). Profiles: `speed` (O3, thin LTO, SIMD — used for measuring), `size` (Os, fat LTO), `scalar` (speed without SIMD). |
| `web/` | Demo page: `?renderer=webgpu\|webgl2` (WebGPU failure → WebGL2, the reason is shown), `?scene=`, `?pkg=`, `?shadows=filter\|analytic`, `?text=outlines\|glyph_run\|atlas`, `?greek=<px>`, `?cull=off`. Drag to pan, scroll to zoom, key `f` = Fit. `window.vello` API for the harness. |
| `serve.mjs` | Loopback static server (ephemeral port) for this directory. |
| `browser.mjs` | Pinned Chromium 156, **full build in new headless mode** (`channel: "chromium"`). |
| `export-board.mjs` | Exports the 12-frame / 4 944-node board of `web/tests/repaint-fixture.mjs` as one world-space scene (frames left → right, 200 px apart) + fonts/images → `out/board/`. |
| `parity.mjs`, `gpu-capture.mjs`, `resources.mjs` | Parity of Canvas2D (Chromium) ↔ `vello_cpu` ↔ `vello_gpu` WebGPU/WebGL2 on the `spikes/native-paint-parity` fixtures. |
| `run.mjs` | Board measurements: encode/submit/GPU-complete at Fit and 100%, rAF pan, init; native `vello_cpu` through the CLI. |
| `export-bench.mjs` | Task 34: export time/peak RSS of the 12 frames through a real server job (`render_frame`) for `vello`/`rust`/`chromium`, cold/warm p50/p95, PNG hashes across starts, comparing compute geometry against `chromium`. |

## Re-run

Needs: the `wasm32-unknown-unknown` target, a project-local `wasm-bindgen` 0.2.129 in `target/wasm-tools/bin/` (or
`WASM_BINDGEN`), the pinned Chromium in `.runtime/browsers` (the harness sets `PLAYWRIGHT_BROWSERS_PATH` itself), `node_modules`.

```bash
cargo test -p fagent-paint-vello
node spikes/vello-gpu/build.mjs --profile all          # pkg/speed, pkg/size, pkg/scalar + sizes.json

# Board: temporary server (SQLite in a temporary directory, port 0, removed when done) — needs a release build because
# the native job still draws each frame's PNG and the debug build exceeds the 45 s deadline.
cargo build --release --locked -p fagent-server
REPAINT_SERVER_BIN=target/release/fagent-server node spikes/vello-gpu/export-board.mjs

node spikes/vello-gpu/parity.mjs --gpu                 # cpu (= the server's `vello` export) / webgpu / webgl2 columns + group table
node spikes/vello-gpu/parity.mjs --gpu --analytic      # shadow via blurred rounded rect
node spikes/vello-gpu/parity.mjs --gpu --atlas         # GPU text via glyph atlas
node spikes/vello-gpu/run.mjs                          # board measurements → out/run-*/summary.json
node spikes/vello-gpu/export-bench.mjs                 # Task 34: export through the server (release) → out/bench-*/summary.json

node spikes/vello-gpu/serve.mjs                        # prints the demo URL to open by hand
```

`parity.mjs` builds `paint-vello-png` (release) itself unless `PAINT_VELLO_BIN` is set. Native CLI:
`paint-vello-png OUT_ROOT REQUEST_JSON PNG_OUTPUT`, request `{scene, scale, shadows?, size?, translate?, cull?,
repeat?, greekBelowPx?}`.

## Environment notes

- **WebGPU in headless macOS:** Playwright's default headless shell only has SwiftShader (WebGPU needs
  `--enable-unsafe-webgpu` and is then a CPU fallback adapter). The full Chromium build in new headless has a real GPU
  (adapter `apple / metal-3`, WebGL2 `ANGLE Metal Renderer: Apple M3 Pro`) with **no flags**; only add
  `--force-color-profile=srgb` as the parity oracle does. The page must be in a secure context (`http://127.0.0.1`).
- Reading WebGPU pixels: `canvas.toDataURL` in the same task as the render (before present).
- GPU-complete: WebGPU waits on `onSubmittedWorkDone` (poll `gpu_idle` with a MessageChannel yield); WebGL2 uses a 1×1
  `readPixels` (Chromium returns from `finish()` early).
- Safari 26: **not run** (no Safari automation in this environment).
- `cargo clippy --target wasm32-unknown-unknown -p fagent-vello-gpu-demo -- -D warnings` hits a pre-existing error in
  `fagent-layout` (`#[expect(clippy::large_enum_variant)]` does not fire on wasm32); adding
  `-A unfulfilled-lint-expectations` makes the demo crate clean.
