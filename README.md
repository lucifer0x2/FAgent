# FAgent

**Not another design tool rebuilt for humans — a design tool built for agents.**

FAgent is a design tool made for AI agents. Agents create, read and edit designs
through [MCP](https://modelcontextprotocol.io); people watch the result live in a read-only
browser viewer. There is no mouse editor, chat or terminal inside the product: the agent is the
author, and the viewer is where you review, inspect, preview and share its work.

A Rust backend owns the design model. Every MCP read is one consistent revision, and every
write is an atomic, idempotent batch, either JSON structured operations or the compact OCD line
DSL, guarded by the revision it was based on. Designs are built from pages, frames and stacks,
tokens with named themes, text and paint styles, components and instances, interaction states,
links and animations. Projects are stored in SQLite (default) or PostgreSQL. Rendering jobs
produce PNG frames and node exports (PNG/SVG) for the agent to check its own work.

## Highlights

- **MCP-first authoring.** About twenty tools cover projects, reads, writes, rendering and
  exports (`tools/list` shows their schemas). `apply_batch` takes up to 500 ordered operations or
  OCD `source`, with `expectedRevision`, a stable `idempotencyKey`, optional page-scoped guards
  (`pageIds`, `strict`) and `dryRun`. A stale write fails with `REVISION_CONFLICT`, the current
  revision and what changed since (`get_changes`).
- **Token-efficient reads and writes.** `get_outline` with a depth, `find_nodes` by name, text,
  kind, ID or parent, scoped `get_subtree`/`get_design_context` (with `compact:true` returning
  only referenced resources), OCD source instead of verbose JSON, assets imported from the
  project `inbox/` by path instead of base64, and `render_frame` that omits the computed scene
  unless asked.
- **Design-system building blocks.** Color, dimension and number tokens with aliases and scoped
  themes, styles, reusable components with flat instance overrides (`get_design_context` lists
  each master once), declared interaction states, prototype links and animations. Authored
  references and resolved values are returned side by side, so code can be written against the
  tokens rather than raw values.
- **GPU viewer powered by [Vello](https://github.com/linebender/vello).** A dedicated Worker runs
  one WASM module that computes frame scenes with the same Rust core as the server and paints
  them on the GPU: WebGPU first, then WebGL2, then `vello_cpu`, then Canvas2D, so the canvas is
  never blank. The main thread only draws a thin overlay for labels and selection.
- **All pages board.** One board with every root frame of every page, grouped in tinted page
  sections with title tabs; click a tab to zoom to that page.
- **Live updates.** The viewer keeps one Server-Sent Events stream per tab; new projects and
  commits from the agent appear without reloading. **Follow agent** (off by default) jumps to the
  project the agent is working on.
- **Motion preview.** **Preview** opens `/preview` for the selected frame and plays its declared
  `props.animations` (load, hover and click triggers, infinite loops), interaction states and
  navigate links in the browser, with Replay and a reduced-motion toggle.
- **Deep links to any element.** `/?project=…&page=…&frame=…&node=…` opens and highlights a
  node, including virtual instance children. **Copy link** (or `Cmd/Ctrl+C`) copies it, and the
  MCP instructions teach agents to resolve such a link with `find_nodes` and
  `get_design_context` instead of guessing.
- **Exports.** Export PNG or SVG from the Inspector or the context menu, or call `export_nodes`
  with scale/width/height settings (or the node's `exportSettings`). SVG keeps live text and
  embeds the exact font bytes.
- **Fonts.** The **Fonts** dialog lists, imports (TTF, OTF, WOFF2) and downloads project fonts.
  Fonts are stored with the project and never installed on the operating system.
- **Public read-only viewer.** An opt-in second listener for sharing through Tailscale Funnel
  serves only projects marked `public`, with an optional shared password, device sessions managed
  through MCP, and live updates off unless enabled.
- **Backups.** The storage layer exports, imports and restores self-contained project bundles
  with a checksummed manifest; the native PostgreSQL deployment has its own backup tooling.

## Performance

Every number below comes from a benchmark recorded in the repository; none was rerun for this
page. They are single runs on one machine, so read them as orders of magnitude, not guarantees.

**Backend, large project.** The [Task24 harness](docs/TASK24-REPORT.md) copies a real
6,711-node project 1× and 30× (30× = 201,330 authored / 599,400 expanded nodes, 570 pages) and
times MCP reads and a one-text-node commit. It ran on an Apple M5 (10 cores, 24 GiB) with a
release build. PostgreSQL 16 ran in a container limited to 2 CPUs / 2 GiB. Values are
**median / p95 ms**. "Before" is the original baseline; "now" is the rerun after the incremental
validation and page-scoped reads of [Task25](docs/TASK25-REPORT.md#task24-rerun-after-slice-4--2026-11-13).

| Operation | Project | SQLite before → now | PostgreSQL before → now |
| --- | --- | --- | --- |
| `get_design_context`, one frame | 1× (6.7k nodes) | 788.8 / 876.2 → **11.3 / 11.5** | 765 / 787.2 → **3.8 / 4.0** |
| `get_design_context`, one frame | 30× (201k nodes) | 42,608.6 / 46,357.8 → **212.4 / 216.4** | 30,426.1 / 34,675.8 → **403.1 / 410.2** |
| `get_outline`, one page | 30× | 1,824 / 2,529.8 → **249.2 / 268.3** | 1,233 / 1,280.8 → **417.8 / 467.8** |
| Commit, strict (`apply_batch`) | 1× | 404.8 / 422 → **63.5 / 65.7** | 377.2 / 389.8 → **98.2 / 129** |
| Commit, strict | 30× | 12,723.7 / 13,348.2 → **2,030.6 / 3,547.4** | 12,639 / 13,294.1 → **2,444.4 / 2,789.7** |
| Commit, page-scoped (`pageIds`) | 30× | 38,871.9 / 40,419.7 → **1,880.9 / 3,403** | 34,121.9 / 55,458.4 → **2,269.4 / 2,646.1** |
| Storage cold snapshot load | 30× | 225.5 / 256.8 | 356.2 / 421.9 |

The 30× responses stay the same size as at 1× (outline 72 KB, context 92 KB): the work grows
with the project, but the answer does not. Write latency at 30× is still above the ~1.5 s target
recorded in the report; validation dominates it.

**Node-memory (experimental, off by default).** [Task28](tasks/28-node-memory-integration.md)
keeps hot projects in RAM (`crates/node-memory`), with indexes updated per operation and the
database still the source of truth. It sits behind three flags, `FAGENT_NODE_MEMORY_SHADOW`,
`FAGENT_NODE_MEMORY_READS` and `FAGENT_NODE_MEMORY_WRITES`. Each defaults to `off`; set one to `on`
to enable it (`strict` is for tests, and `WRITES=on` also turns on reads and the mirror). The figures below were measured on a
generated 200k-node fixture with PostgreSQL and a release build; the source does not record the
hardware. This fixture differs from the Task24 project, so do not compare it with the table
above.

| Measurement (PostgreSQL, 200k-node fixture) | Flag off | Flag on |
| --- | --- | --- |
| `get_design_context`, one frame (`READS`) | 156 ms cached; 207 ms right after a write | **1.5 ms** warm; **2.0 ms** right after a write |
| Page-scoped commit, median of 30 (`WRITES`) | 636 ms (load 200, persist 34) | **468 ms** (load 29, persist 3) |
| Same commit, real project of 7,025 nodes | 28.5 ms | **22.5 ms** |
| Shadow mirror cost per commit (`SHADOW`) | – | 114 µs (17–26 µs on real projects); first load 76 ms |

In-memory operations on the 200k fixture take well under a millisecond: a props update takes
584 ns, and a begin + commit takes 542 ns. Memory use is about 1.3 KB of RAM per authored node.
Later node-memory stages (persistent reader indexes, delta validation, HOT reader cleanup) have
been implemented, but their benchmarks are deferred. See the
[I3b](docs/TASK28-I3B-REPORT.md), [I4](docs/TASK28-I4-REPORT.md) and
[I5](docs/TASK28-I5-REPORT.md) reports and the
[base verification](docs/FAGENT-BASE-VERIFICATION.md).

**Viewer.** These numbers come from an Apple M3 Pro with a production build
([details](docs/VIEWER.md#measurements--2026-11-17)). On a board of 12 frames and 4,945 nodes,
the Vello engine reaches "Scene rN" in 1.8–1.9 s, against 2.8–3.6 s for Canvas2D in the same run.
Panning at Fit draws in 13.3 / 13.8 ms (p50 / p95). No main-thread task exceeded 50 ms while
panning or zooming.

## Architecture

```text
agent (MCP) / browser ──► fagent-server  HTTP API · MCP · SSE · serves web/dist and the
                            │            hash-named, immutable /engine-wasm module
                            ├─► fagent-storage  SQLite | PostgreSQL (+ node-memory behind a flag)
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
| `crates/node-memory` | In-memory node indexes (`FAGENT_NODE_MEMORY_*`, off by default) |
| `crates/scene-input` | Authored models, validation and scene input preparation |
| `crates/layout` | Taffy layout with text measurement (also the chromium worker's `layout-step`) |
| `crates/text` | Font loading, HarfRust shaping and glyph outlines shared by measure and paint |
| `crates/paint-core` | Backend-neutral paint contract: wire scene, validation, strict resources, image decode/ICC, PNG encode |
| `crates/paint-vello` | scene-3 encoded for Vello: `vello_cpu` export and the viewer's `vello_gpu` path |
| `crates/scene-wasm` | Snapshot → resolved scene in the browser |
| `crates/engine-wasm` | Viewer engine Worker: scene-wasm compute + paint-vello + `vello_gpu`/`vello_cpu` |

The server's `vello` export and the viewer share `fagent-layout`, `fagent-text` and
`fagent-paint-vello`; the former tiny-skia painter (`crates/render`) was removed. Details:
[docs/VIEWER.md](docs/VIEWER.md), [docs/SERVER.md](docs/SERVER.md#render-backends-render_backend),
[Task 34](tasks/34-vello-cpu-export-unify.md).

## Local setup

The verified development environment is Rust/Cargo 1.99.1, Node 24.21.0, npm 11.19.0 and Playwright Chromium 156.0.8078.4. Dependencies are pinned in the lockfiles. From this repository:

```sh
npm ci
npm --prefix web ci
export PLAYWRIGHT_BROWSERS_PATH="$PWD/.runtime/browsers"
npx playwright install chromium
```

The first installation needs registry/network access. Build and launch the whole app with either profile:

| Profile | Build the renderer, Rust server/layout and viewer | Launch the existing build |
| --- | --- | --- |
| Debug | `npm run build` | `npm run dev` |
| Release (optimized Rust binaries) | `npm run build:release` | `npm start` |

Both launch commands run the Rust server and serve the compiled `web/dist` viewer. They reuse existing artifacts without rebuilding. A missing artifact reports the matching build command. The build commands require the installed root/web Node dependencies and Rust toolchain; they never install dependencies or Chromium. With Cargo dependencies cached, append `-- --offline` to either build command. The viewer build may report a JavaScript chunk-size warning.

The browser WASM modules are built separately (Docker builds them automatically). They need the
`wasm32-unknown-unknown` target and wasm-bindgen 0.2.129:

```sh
npm run build:scene-wasm    # frame scene computation in a browser Worker (Canvas2D path)
npm run build:engine-wasm   # Vello engine: scene computation and GPU painting
```

Without the engine module the viewer falls back to Canvas2D with a notice. See
[browser scene build, parity and timings](docs/VIEWER-WASM-SCENE.md) and the
[Vello engine](docs/VIEWER.md#vello-engine-renderervello).

For example, build and run the optimized app with an isolated data root and a free port:

```sh
npm run build:release
npm start -- --port 0 --data-root /path/to/separate-data
```

By default, open [the local viewer](http://127.0.0.1:3847); with `--port 0`, use the URL printed at startup. Starting the server creates no demo project. The backend launches the Node/Chromium rendering worker when needed and listens only on loopback. Stop it with Ctrl-C: open event streams are told to end, and a second Ctrl-C (or three seconds) leaves immediately. Keep the repository's built renderer, layout binary, worker and browser installation available while running.

`CARGO_TARGET_DIR` selects the Cargo output directory for both building and launching; set it consistently for both commands. Relative values resolve from the repository root. Launchers select the server and layout worker from the same `debug` or `release` directory and pass the layout binary through `FAGENT_LAYOUT_BIN`. They default `PLAYWRIGHT_BROWSERS_PATH` to the repository's `.runtime/browsers`; an explicit environment value wins. The viewer and renderer bundles are shared by both profiles, so rebuilding either profile refreshes these bundles.

`PORT` selects the local listener (default 3847), and `--port NUMBER` takes precedence. Both accept 0 to bind an available port and print it at startup.

Contributors: enable the public-content hook with `git config core.hooksPath .githooks` and install [gitleaks](https://github.com/gitleaks/gitleaks) (`brew install gitleaks`) so commits are also scanned for secrets.

For optional frontend hot reload, keep the backend running and run `npm --prefix web run dev` separately. Its `/api` proxy defaults to port 3847; set `FAGENT_API_URL` for another backend URL. See [viewer behavior](docs/VIEWER.md) and the [HTTP contract](docs/CONTRACT.md).

## Viewer and render backends

The viewer paints with the Vello engine by default (WebGPU, then WebGL2, `vello_cpu`, Canvas2D);
the status bar shows the active backend. `?renderer=canvas2d` selects the Canvas2D painter, and
`?scene=server` computes scenes on the server (Canvas2D only). With Canvas2D, browser scene
failures fall back to the server. `FAGENT_VIEWER_OVERSCAN` (server environment, percent per side,
0–50, default 0 = off) makes the Vello engine draw that much beyond each viewport edge, so a pan
during a long frame computation slides in drawn pixels instead of a blank strip; an invalid value
stops startup. See [Overscan](docs/VIEWER.md).

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
`vello` (see [Native runtime](docs/NATIVE-RUNTIME.md)). Details:
[render backends](docs/SERVER.md#render-backends-render_backend) and
[Task 34](tasks/34-vello-cpu-export-unify.md).

## Export backends: Chromium vs Vello

Numbers from the Task 34 measurements ([PURE-RUST-EXPORT.md § Task 34](docs/PURE-RUST-EXPORT.md#task-34--export-vello_cpu)):
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
  ([Task 23](tasks/23-implementation-conformance.md)) or a pixel diff that ignores anti-aliasing
  is less sensitive to the renderer either way.

`RENDER_BACKEND=chromium|vello` overrides a build's default at startup, provided that build
ships the runtime the backend needs.

## Storage

`DATABASE_PROVIDER` accepts `sqlite` (default) or `postgresql`. SQLite uses `~/.fagent` by default. Set `FAGENT_DATA_ROOT` or pass `--data-root DIR` to override it. Each SQLite project uses `<data-root>/<slug>/db.sql` (SQLite binary) and `assets/`. Only `create_project` creates a project. Use a separate temporary data root for SQLite experiments and tests.

For PostgreSQL, set `DATABASE_URL` to your connection string in the environment, then launch the built app:

```sh
DATABASE_PROVIDER=postgresql PORT=3850 npm start
```

Optional PostgreSQL limits (positive integers; unset keeps the default; an invalid value stops startup and names the variable):

| Variable | Default | Range |
| --- | --- | --- |
| `DATABASE_POOL_SIZE` | 4 connections | 1–32 |
| `DATABASE_CONNECT_TIMEOUT_MS` | 3000 | 1–2147483647 ms |
| `DATABASE_STATEMENT_TIMEOUT_MS` | 15000 | 1–2147483647 ms |
| `DATABASE_LOCK_TIMEOUT_MS` | 3000 | 1–2147483647 ms |
| `DATABASE_OPERATION_TIMEOUT_MS` | 30000 (pool wait per operation; also idle-in-transaction and TCP user timeouts) | 1–2147483647 ms |

`FAGENT_DATABASE_SCHEMA` selects the schema that holds the runtime tables (default `fagent_runtime_v1`, for example `public`); it must be 1–63 lowercase letters, digits or `_`, starting with a letter or `_`.

`npm run dev` accepts the same environment variables. PostgreSQL uses an in-process snapshot cache with a one-hour TTL; it reads only `import_asset`/`import_assets` and `apply_batch_file` files from `<data-root>/<slug>/inbox` and stores nothing in the data root. Changing providers selects a different store. SQLite projects stay in the data root and require a separate migration to appear in PostgreSQL. See the [PostgreSQL adapter](docs/POSTGRES-BACKEND.md) for database permissions and transport details, and [backup/restore](docs/BACKUP.md) for project bundles.

## Connect an agent

Configure an MCP client with **Streamable HTTP** and URL `http://127.0.0.1:3847/mcp` (adjust the port if needed). This is an HTTP endpoint, not a stdio command. A minimal connection entry is:

```json
{ "url": "http://127.0.0.1:3847/mcp" }
```

The surrounding configuration format depends on the client. The client initializes a session and retains the negotiated MCP session/protocol headers. The checked transport negotiates protocol `2025-11-25`.

Use `list_projects`/`create_project`, then `get_outline` before scoped `get_subtree` or `get_design_context`. Resource metadata is available through `list_tokens`, `list_styles`, `list_assets` and `list_fonts`. `import_asset` takes one asset (10 MiB) from a file in the project's `inbox/`, bounded base64, or another project's asset or font (`sourceProjectId` with `assetId` or `fontId`); it never fetches URLs. `import_assets` takes 1–50 such items (64 MiB in total) and commits them as one revision. `apply_batch_file` applies a file from the inbox: `.ocd` OCD source or a JSON operation array (recommended for large or iterative batches: dry-run, fix the reported line, apply). `apply_batch` takes a `projectId`, `expectedRevision`, stable `idempotencyKey` and ordered operations or OCD `source` (read `skill://fagent/ocd.md` through MCP). Reuse the exact body/key after a lost response; read back after a revision conflict.

Create a page, then a frame, then its descendants. Text must declare a valid font (the bundled font is `font_bundled`), size and line height, or obtain them from a valid text style. `render_frame` returns a job; poll it by `jobId`, inspect its exact revision/provenance and obtain the PNG through `imageUrl`. The response omits the computed scene unless you pass `includeScene: true`; `inspect_layout` reads the bounds and text lines for that job's scene key. `export_nodes` returns PNG/SVG files of up to eight nodes. Available tools expose their schemas through `tools/list`.

To implement an existing design in code rather than author one, read `get_outline` for the frame,
then `get_design_context` for that `frameId`. Nodes carry authored `props` beside `resolvedProps`:
the authored side holds the token and style references the design was built from, the resolved
side what they evaluate to for one theme, so code written against the reference survives a theme
change. Build the structure the design declares — a container's layout `mode` is `row`, `column`
or `free`, with `padding` and `gap` — instead of deriving it from measured bounds; those bounds
and the wrapped text lines belong to one viewport and are there to check the result. The design
can declare named token themes, flat visual `states`, `links` and `animations`.
Context can resolve declared states, `render_frame` previews a selected state, and HTML
preview plays declared links and motion. These declarations do not supply application data,
screen transitions or arbitrary layout/font variants.

Read the generated [`props` resource](skill://fagent/props.md) and
`fagent://capabilities` through MCP `resources/read` before approximating an
unsupported property. Capabilities distinguish authored keys, Canvas/PNG, HTML preview
and SVG; cache by fingerprint/version, and inspect runtime job readiness separately.
Author one full semantic string per paragraph/label, preserving spaces and intentional
hard breaks. Read that authored source when implementing; measured line/character positions
are comparison evidence at one viewport. Intentional lettering, wordmarks and per-letter
motion remain valid: record their intent with a node note. Text diagnostics are advisory,
scoped and bounded; they never merge or rewrite layers automatically.

## Share a read-only viewer

To share the viewer through Tailscale Funnel, use the separate, opt-in
[public read-only listener](docs/SERVER.md#public-viewer-through-tailscale-funnel).
It exposes assets, fonts, downloads and exports only for projects
marked `public`. Live updates there are off by default (a reload shows newer
changes); set `FAGENT_PUBLIC_LIVE_UPDATES=on` to enable them. Projects start `private`. Ask your agent to call
`update_project` with `{"projectId":"<id>","privacy_status":"public"}` to share
a project, or pass `"private"` to withdraw it. Set a shared viewer password through the local MCP tool
`set_public_viewer_password` to require sign-in, and use
`list_public_viewer_sessions` and `revoke_public_viewer_session` to manage
devices. MCP, project creation and font import stay on the local listener.
Funnel must target the public port (3848 by default, 3858 with PostgreSQL
Compose). See [password and proxy setup](docs/SERVER.md#public-viewer-password).

## Docker

The default (`chromium`) image bundles the release server, layout-step, worker, renderer, viewer and Playwright's Chromium headless shell (Rust 1.99.1 and Node 24.21.0 on Debian trixie); no local toolchain is needed:

```sh
docker build -t fagent:local .
docker compose up -d
node docker/smoke.mjs http://127.0.0.1:3847   # optional: creates a project smoke-<time>
```

The viewer is at `http://127.0.0.1:3847` and MCP at `http://127.0.0.1:3847/mcp`. Projects live in the named volume `fagent-data` mounted at `/data`; the container never sees `~/.fagent`. To keep them in a host directory instead, replace `fagent-data:/data` in `compose.yaml` with a bind mount such as `./fagent-data:/data`, writable by uid 1000 (the `node` user of the chromium image, `fagent` of the vello image). `docker compose stop` sends SIGINT, which shuts the server down gracefully.

Inside the container the server listens on `0.0.0.0` (`--host 0.0.0.0`), and `compose.yaml` publishes it on `127.0.0.1` only. Keep it that way: the API has no authentication, and the loopback Host check stops browsers, not other machines. `FAGENT_PORT=3850 docker compose up -d` moves the host and container port together, so `imageUrl` and the Origin check match the address you use. With plain `docker run`, use `--init -p 127.0.0.1:3847:3847 -v fagent-data:/data --shm-size 512m`. Do not run the native server and the container on the same data root at the same time: the data-root lock is not guaranteed to work across a bind mount.

The default Compose file runs SQLite. [Local PostgreSQL in Docker Compose](docs/COMPOSE-POSTGRES.md) runs the app and PostgreSQL in two containers, with a separate data volume and the optional read-only viewer.

The image's render backend is a build argument: `FAGENT_RENDER_BACKEND=vello docker compose build`
(or `docker build --build-arg FAGENT_RENDER_BACKEND=vello`) makes an image without Node, Playwright
or Chromium (a `debian:trixie-slim` runtime with the server and viewer only); the default build is
`chromium`. Compose passes `RENDER_BACKEND` into the container only when it is set in the env
file or shell, otherwise the image's default applies. The public-viewer overlay preserves this
choice. Recreate the app container after changing it; the browser viewer is unaffected by this
setting. The Compose healthchecks use bash, so they work with either image.

For the isolated local PostgreSQL deployment on OrbStack, see [Local Kubernetes](docs/KUBERNETES.md). It builds a separate image, keeps database data in a PVC and exposes the viewer/MCP only through loopback port-forward on port 3857.
The opt-in [Kubernetes public viewer](docs/KUBERNETES-PUBLIC-VIEWER.md) adds a separate read-only listener on loopback 3858 for later user-managed Funnel.

## Verification

```sh
npm run test:e2e
```

This builds the required artifacts using cached Cargo dependencies, starts the production Rust server on a random loopback port with a temporary data root, and drives MCP plus Chromium. It terminates the test processes and removes the temporary project afterward. A sandbox must permit loopback listeners and Chromium. The shorter `node spikes/e2e/run.mjs` reuses existing builds.

The live acceptance run verifies MCP creation/import, SQLite structure, typed authored/resolved readback, exact scene and PNG provenance, viewer/PNG pixel equality, token updates through production SSE, narrow viewport wrapping, invalid-batch rollback, idempotent retry, and persistence through backend restart and browser reload. It also checks that viewer API traffic uses GET only. See [the E2E protocol and limitations](spikes/e2e/README.md). Each run writes its machine-readable evidence, PNG and viewer screenshot to `artifacts/e2e/` (not versioned).

Targeted module checks remain separate:

```sh
npm run check:renderer
npm run test:pipeline
npm --prefix web run typecheck
npm --prefix web test
cargo test -p fagent-storage --offline --features fault-injection -- --test-threads=2
cargo test -p fagent-server --offline -- --test-threads=2
```

The live E2E covers one small project on the recorded Chromium runtime; it does not establish cross-platform pixel identity or every failure/recovery case. Milestone acceptance is tracked separately in [docs/ACCEPTANCE.md](docs/ACCEPTANCE.md).

## Documentation

| Topic | Document |
| --- | --- |
| Product requirements and acceptance criteria | [PLAN.md](PLAN.md), [docs/ACCEPTANCE.md](docs/ACCEPTANCE.md) |
| MCP tools, HTTP API, jobs, SSE, public viewer | [docs/SERVER.md](docs/SERVER.md) |
| Viewer ↔ HTTP API contract and data types | [docs/CONTRACT.md](docs/CONTRACT.md) |
| Browser viewer, Vello engine, All pages, motion preview | [docs/VIEWER.md](docs/VIEWER.md) |
| Large-project benchmarks and node-memory | [docs/TASK24-REPORT.md](docs/TASK24-REPORT.md), [docs/TASK25-REPORT.md](docs/TASK25-REPORT.md), [tasks/28](tasks/28-node-memory-integration.md) |
| Tokens, aliases and themes | [docs/RESOLUTION.md](docs/RESOLUTION.md) |
| Components and instances | [docs/COMPONENTS.md](docs/COMPONENTS.md) |
| Fonts | [docs/FONTS.md](docs/FONTS.md) |
| Project bundles, backup and restore | [docs/BACKUP.md](docs/BACKUP.md) |
| Native macOS runtime and PostgreSQL backups | [docs/NATIVE-RUNTIME.md](docs/NATIVE-RUNTIME.md), [docs/NATIVE-BACKUP.md](docs/NATIVE-BACKUP.md) |
| Storage benchmark | [spikes/storage/README.md](spikes/storage/README.md) |

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT)
at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without
any additional terms or conditions.

Bundled fonts keep their own licenses: Noto Sans ([renderer/fonts/OFL.txt](renderer/fonts/OFL.txt))
and the Lato test fixtures ([fixtures/fonts/Lato-OFL.txt](fixtures/fonts/Lato-OFL.txt)) are under
the SIL Open Font License 1.1.
