# Getting started

How to build, run, connect, store, share and verify FAgent. The README has the short version.

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
[browser scene build, parity and timings](VIEWER-WASM-SCENE.md) and the
[Vello engine](VIEWER.md#vello-engine-renderervello).

For example, build and run the optimized app with an isolated data root and a free port:

```sh
npm run build:release
npm start -- --port 0 --data-root /path/to/separate-data
```

By default, open [the local viewer](http://127.0.0.1:3847); with `--port 0`, use the URL printed at startup. Starting the server creates no demo project. The backend launches the Node/Chromium rendering worker when needed and listens only on loopback. Stop it with Ctrl-C: open event streams are told to end, and a second Ctrl-C (or three seconds) leaves immediately. Keep the repository's built renderer, layout binary, worker and browser installation available while running.

`CARGO_TARGET_DIR` selects the Cargo output directory for both building and launching; set it consistently for both commands. Relative values resolve from the repository root. Launchers select the server and layout worker from the same `debug` or `release` directory and pass the layout binary through `FAGENT_LAYOUT_BIN`. They default `PLAYWRIGHT_BROWSERS_PATH` to the repository's `.runtime/browsers`; an explicit environment value wins. The viewer and renderer bundles are shared by both profiles, so rebuilding either profile refreshes these bundles.

`PORT` selects the local listener (default 3847), and `--port NUMBER` takes precedence. Both accept 0 to bind an available port and print it at startup.

Contributors: enable the public-content hook with `git config core.hooksPath .githooks` and install [gitleaks](https://github.com/gitleaks/gitleaks) (`brew install gitleaks`) so commits are also scanned for secrets.

For optional frontend hot reload, keep the backend running and run `npm --prefix web run dev` separately. Its `/api` proxy defaults to port 3847; set `FAGENT_API_URL` for another backend URL. See [viewer behavior](VIEWER.md) and the [HTTP contract](CONTRACT.md).

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

`npm run dev` accepts the same environment variables. Both providers keep hot projects in node memory (idle projects are dropped after one hour); PostgreSQL reads only `import_asset`/`import_assets` and `apply_batch_file` files from `<data-root>/<slug>/inbox` and stores nothing in the data root. Changing providers selects a different store. SQLite projects stay in the data root and require a separate migration to appear in PostgreSQL. See the [PostgreSQL adapter](POSTGRES-BACKEND.md) for database permissions and transport details, and [backup/restore](BACKUP.md) for project bundles.

## Share a read-only viewer

To share the viewer through Tailscale Funnel, use the separate, opt-in
[public read-only listener](SERVER.md#public-viewer-through-tailscale-funnel).
It exposes assets, fonts, downloads and exports only for projects
marked `public`. Live updates there are off by default (a reload shows newer
changes); set `FAGENT_PUBLIC_LIVE_UPDATES=on` to enable them. Projects start `private`. Ask your agent to call
`update_project` with `{"projectId":"<id>","privacy_status":"public"}` to share
a project, or pass `"private"` to withdraw it. Set a shared viewer password through the local MCP tool
`set_public_viewer_password` to require sign-in, and use
`list_public_viewer_sessions` and `revoke_public_viewer_session` to manage
devices. MCP, project creation and font import stay on the local listener.
Funnel must target the public port (3848 by default, 3858 with PostgreSQL
Compose). See [password and proxy setup](SERVER.md#public-viewer-password).

## Docker

The default (`chromium`) image bundles the release server, layout-step, worker, renderer, viewer and Playwright's Chromium headless shell (Rust 1.99.1 and Node 24.21.0 on Debian trixie); no local toolchain is needed:

```sh
docker build -t fagent:local .
docker compose up -d
node docker/smoke.mjs http://127.0.0.1:3847   # optional: creates a project smoke-<time>
```

The viewer is at `http://127.0.0.1:3847` and MCP at `http://127.0.0.1:3847/mcp`. Projects live in the named volume `fagent-data` mounted at `/data`; the container never sees `~/.fagent`. To keep them in a host directory instead, replace `fagent-data:/data` in `compose.yaml` with a bind mount such as `./fagent-data:/data`, writable by uid 1000 (the `node` user of the chromium image, `fagent` of the vello image). `docker compose stop` sends SIGINT, which shuts the server down gracefully.

Inside the container the server listens on `0.0.0.0` (`--host 0.0.0.0`), and `compose.yaml` publishes it on `127.0.0.1` only. Keep it that way: the API has no authentication, and the loopback Host check stops browsers, not other machines. `FAGENT_PORT=3850 docker compose up -d` moves the host and container port together, so `imageUrl` and the Origin check match the address you use. With plain `docker run`, use `--init -p 127.0.0.1:3847:3847 -v fagent-data:/data --shm-size 512m`. Do not run the native server and the container on the same data root at the same time: the data-root lock is not guaranteed to work across a bind mount.

The default Compose file runs SQLite. [Local PostgreSQL in Docker Compose](COMPOSE-POSTGRES.md) runs the app and PostgreSQL in two containers, with a separate data volume and the optional read-only viewer.

The image's render backend is a build argument: `FAGENT_RENDER_BACKEND=vello docker compose build`
(or `docker build --build-arg FAGENT_RENDER_BACKEND=vello`) makes an image without Node, Playwright
or Chromium (a `debian:trixie-slim` runtime with the server and viewer only); the default build is
`chromium`. Compose passes `RENDER_BACKEND` into the container only when it is set in the env
file or shell, otherwise the image's default applies. The public-viewer overlay preserves this
choice. Recreate the app container after changing it; the browser viewer is unaffected by this
setting. The Compose healthchecks use bash, so they work with either image.

For the isolated local PostgreSQL deployment on OrbStack, see [Local Kubernetes](KUBERNETES.md). It builds a separate image, keeps database data in a PVC and exposes the viewer/MCP only through loopback port-forward on port 3857.
The opt-in [Kubernetes public viewer](KUBERNETES-PUBLIC-VIEWER.md) adds a separate read-only listener on loopback 3858 for later user-managed Funnel.

## Verification

```sh
npm run test:e2e
```

This builds the required artifacts using cached Cargo dependencies, starts the production Rust server on a random loopback port with a temporary data root, and drives MCP plus Chromium. It terminates the test processes and removes the temporary project afterward. A sandbox must permit loopback listeners and Chromium. The shorter `node spikes/e2e/run.mjs` reuses existing builds.

The live acceptance run verifies MCP creation/import, SQLite structure, typed authored/resolved readback, exact scene and PNG provenance, viewer/PNG pixel equality, token updates through production SSE, narrow viewport wrapping, invalid-batch rollback, idempotent retry, and persistence through backend restart and browser reload. It also checks that viewer API traffic uses GET only. See [the E2E protocol and limitations](../spikes/e2e/README.md). Each run writes its machine-readable evidence, PNG and viewer screenshot to `artifacts/e2e/` (not versioned).

Targeted module checks remain separate:

```sh
npm run check:renderer
npm run test:pipeline
npm --prefix web run typecheck
npm --prefix web test
cargo test -p fagent-storage --offline --features fault-injection -- --test-threads=2
cargo test -p fagent-server --offline -- --test-threads=2
```

The live E2E covers one small project on the recorded Chromium runtime; it does not establish cross-platform pixel identity or every failure/recovery case. Milestone acceptance is tracked separately in [docs/ACCEPTANCE.md](ACCEPTANCE.md).
