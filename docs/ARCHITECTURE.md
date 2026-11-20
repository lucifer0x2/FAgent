# Architecture

How the server, storage, render jobs and the browser viewer fit together, and how to choose a render backend.

```text
agent (MCP) / browser ──► fagent-server  HTTP API · MCP · SSE · serves web/dist and the
                            │            hash-named, immutable /engine-wasm module
                            ├─► fagent-storage  SQLite | PostgreSQL (+ node-memory, always on)
                            └─► export jobs: queue → admission → runner, backend fixed at build time
                                  ├─ vello:    in process  fagent-layout → fagent-text →
                                  │            fagent-paint-vello (vello_cpu) → PNG via paint-core
                                  └─ chromium: Node worker → layout-step + Playwright Chromium
                                               (renderer/ Canvas2D) → PNG
browser viewer (web/) ──► engine-wasm in a Worker: scene-wasm → fagent-paint-vello →
                          vello_gpu WebGPU → WebGL2 → vello_cpu → Canvas2D fallback
```

| Crate | Role |
| --- | --- |
| `crates/server` (`fagent-server`) | HTTP/MCP/SSE, jobs (render, export), viewer static files, public viewer |
| `crates/storage` | SQLite or PostgreSQL storage, revisions, assets and fonts |
| `crates/node-memory` | `NodeMemory`: in-memory engine for hot projects, no database knowledge; every latest-revision read (scoped and full snapshots) and every batch goes through it. Full validation once per load (admission); each batch is validated on its delta (per-op checks, then the end-of-batch rules of `scene-input` `validate::delta`). Storage keeps identity and freshness, persists the rows a batch returns and reads past revisions from the database; there is no storage snapshot cache |
| `crates/scene-input` | Authored models, validation and scene input preparation |
| `crates/layout` | Taffy layout with text measurement (also the chromium worker's `layout-step`) |
| `crates/text` | Font loading, HarfRust shaping and glyph outlines shared by measure and paint |
| `crates/paint-core` | Backend-neutral paint contract: wire scene, validation, strict resources, image decode/ICC, PNG encode |
| `crates/paint-vello` | scene-3 encoded for Vello: `vello_cpu` export and the viewer's `vello_gpu` path |
| `crates/scene-wasm` | Snapshot → resolved scene in the browser |
| `crates/engine-wasm` | Viewer engine Worker: scene-wasm compute + paint-vello + `vello_gpu`/`vello_cpu` |

The server's `vello` export and the viewer share `fagent-layout`, `fagent-text` and
`fagent-paint-vello`; the former tiny-skia painter (`crates/render`) was removed. Details:
[docs/VIEWER.md](VIEWER.md), [docs/SERVER.md](SERVER.md#render-backends-render_backend),
[Task 34](../tasks/34-vello-cpu-export-unify.md).

## Viewer and render backends

The viewer paints with the Vello engine by default (WebGPU, then WebGL2, `vello_cpu`, Canvas2D);
the status bar shows the active backend. `?renderer=canvas2d` selects the Canvas2D painter, and
`?scene=server` computes scenes on the server (Canvas2D only). With Canvas2D, browser scene
failures fall back to the server. `FAGENT_VIEWER_OVERSCAN` (server environment, percent per side,
0–50, default 0 = off) makes the Vello engine draw that much beyond each viewport edge, so a pan
during a long frame computation slides in drawn pixels instead of a blank strip; an invalid value
stops startup. See [Overscan](VIEWER.md).

PNG rendering and exports run on the server with one of two render backends. `chromium`: a Node
worker drives headless Chromium with the shared Canvas2D renderer. `vello`: the server computes
in process and paints with `vello_cpu`, the viewer's encoder, without Node, Chromium or
layout-step (pixels differ slightly from Chrome). The default is chosen **at build time** with
`FAGENT_RENDER_BACKEND=chromium|vello` (default `chromium`), read by `npm run build`,
`npm run build:release`, `scripts/native/build-release.sh` and the Docker build:

```sh
npm run build                                       # chromium: also renderer bundle + layout-step
FAGENT_RENDER_BACKEND=vello npm run build:release   # vello: no renderer bundle or layout-step
```

A `vello` build needs no Node or Chromium to run (Node still builds the viewer).
`RENDER_BACKEND` or `--render-backend` overrides the default at startup; asking a `vello` build
for `chromium` without its worker runtime fails at startup. The native release on this Mac uses
`vello` (see [Native runtime](NATIVE-RUNTIME.md)). Details:
[render backends](SERVER.md#render-backends-render_backend) and
[Task 34](../tasks/34-vello-cpu-export-unify.md).

## Export backends: Chromium vs Vello

Numbers from the Task 34 measurements ([PURE-RUST-EXPORT.md § Task 34](PURE-RUST-EXPORT.md#task-34--export-vello_cpu)):
a 12-frame board (4 944 nodes) through real server jobs, on a moderately loaded machine, so they
compare the two backends rather than set a baseline.

| | `vello` (`FAGENT_RENDER_BACKEND=vello`; native release default) | `chromium` (default build, Docker/Compose) |
| --- | --- | --- |
| Runtime | One binary + `web/dist` + engine WASM; no Node, Playwright or Chromium | Node, Playwright Chromium, `worker/`, `renderer/dist`, `layout-step` |
| Peak RAM (server process tree) | ~324 MiB | ~1.4 GiB (1 393 MiB) |
| Server start | ~102 ms | ~303 ms |
| Board, cold (p50) | ~1 972 ms | ~2 501 ms |
| Frame, warm (p50 / p95) | ~125 / 162 ms | ~132 / 151 ms |
| Pixels match | What the FAgent viewer draws: the same layout → text → `paint-vello` path | What a Chromium browser draws (Canvas2D in headless Chromium) |

**Two renderers, one design.** The backends differ the way two browser engines do (Firefox's
WebRender and Chromium's Skia drawing the same page): geometry, colour, layout and text
positions are the same; the differences are at the level of anti-aliasing, text rasterization
and numeric precision. Measured against Chromium, 44 of 75 non-text parity cases stay under the
strict gate (< 0.5% of pixels differing by more than 8/255); the others differ in low-alpha
gradient and shadow tails, stroke edges at scale 1 and image sampling (0.5–1.8% of pixels on
white). Text differs in up to ~6.6% of pixels (Vello outlines vs CoreText glyphs), with identical
line positions and geometry.

Choosing:

- `vello` exports what the FAgent viewer shows, with no Node or Chromium at runtime, about a
  quarter of the memory and a faster cold start. The raster is single-threaded and CPU-only
  (a GPU export, Task 34 phase 5, was not pursued: raster is not the bottleneck); the warm p95
  was slightly above Chromium's in this run.
- `chromium` exports what a Chromium browser draws, so it is the closer reference when an agent's
  implemented HTML/CSS is scored against the design by raw pixel diff (for example "> 90%
  match"), where a 1–2% difference (text up to ~6.6%) can count. It needs the Node worker,
  Playwright Chromium and their artifacts at runtime. Element-level comparison
  ([Task 23](../tasks/23-implementation-conformance.md)) or a pixel diff that ignores anti-aliasing
  is less sensitive to the renderer either way.

`RENDER_BACKEND=chromium|vello` overrides a build's default at startup, provided that build
ships the runtime the backend needs.
