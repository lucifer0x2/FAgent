# Task 47 — Native desktop app: viewer and optional in-process server

**Priority (user, 2026-11-28):** new product track; not ordered against Tasks 31–46 yet.
**Status:** Not assigned. Packaged 2026-11-28 from a read-only survey of `main` at `50bce86`; user decisions of 2026-11-28 are folded in (see "Decisions").
**Depends on:** nothing merged first. Uses the Vello engine (Tasks 30/34/42) and node memory (Task 41) as they are on `main`.
**Owned scope:**
- New crate `crates/desktop` (binary `fagent-desktop`): window, tabs, tray, settings, bottom bar, viewer UI, native engine host, data sources.
- `crates/server`: a public embedding API (typed storage options and probes, port probe, listener start/stop, a way to stop a `Jobs` runner) and an `embedded` read module for the in-process viewer. `src/main.rs` and `src/startup/*` move onto that API with no behavior change.
- `crates/server/src/http.rs`: one additive field on `GET /api/v1/viewer` (see "Port probe"), documented in `docs/CONTRACT.md` and `docs/SERVER.md`.
- `crates/engine-wasm`: only what a native host needs (a target-neutral host boundary, or moving `web/` glue behind it). The wasm build, its API and `web/` behavior stay unchanged. A crate split (for example `engine-core` + `engine-wasm`) is allowed if the wasm build stays identical; record the decision here.
- `crates/storage`: read-only probes (SQLite root, PostgreSQL connection) and, if needed, a narrow public read API for the in-process viewer. No change to node memory semantics or to `Store::open` / `PostgresBackend::with_config` behavior.
- Docs: a new `docs/DESKTOP.md`, `docs/ARCHITECTURE.md` (diagram + crate table), `README.md` (one short section), this file, `docs/agents/LOG.md`.
- Not owned: MCP tool behavior, the rest of the HTTP API contract, the web viewer (`web/`), export backends, the Chromium worker.
**Workspace:** a new branch `feat/native-desktop` from `main`. Do not push or open a PR unless the user asks.

## Context

Today there is one binary, `fagent-server` (`crates/server/src/main.rs`, ~100 lines): it reads options (`startup::Startup::from_env`), opens storage (`StorageOptions::{Sqlite, Postgres}::open`), builds `AppState::new(storage, config)` and serves `router(state)` with axum on `127.0.0.1:3847`. The viewer is the browser app in `web/`, which runs `engine-wasm` in a Worker and reads the HTTP API and SSE.

The user wants a native desktop app where the viewer and the server can run in **one process**, sharing one `Storage` (so one `NodeMemory`), while every piece stays optional so the viewer can also work against a server elsewhere.

Facts the design depends on (checked against the code):

- **Server library.** `lib.rs` exports `router`, `AppState`, `Config`, `RenderBackend`, `PublicViewer`. `StorageOptions` and all option parsing live in the binary-only `startup` module, not in the library. `AppState` is `Clone`; `begin_shutdown()` / `shutting_down()` drive graceful stop.
- **`Config` is fixed per `AppState`.** `Config::for_backend(workspace, port, backend)` bakes `base_url = http://127.0.0.1:{port}` into an `Arc<Config>` that `AppState` and the `Jobs` runner (`Jobs::start`, started inside `AppState::new`) both hold. `base_url` is used for MCP CORS origins, font/asset/job/export URLs and the origin guard. A different port needs a new `AppState` (new `Config`, new `Jobs`, new `events` channel); it can sit over the **same** `Storage` clone. `Jobs` has no stop method today. `AppState::new` also needs a tokio runtime (`Jobs`).
- **Render backend.** The desktop must use `RenderBackend::Vello` (no Node/Chromium). `Config::for_backend(.., Vello)` fingerprints the current executable (panics if unreadable).
- **Static viewer.** `viewer_routes` serves `<workspace>/web/dist`; a missing `dist` only 404s, it does not fail startup.
- **Local listener is loopback-only by design.** The request guard (`http.rs`) accepts only `Host: localhost|127.0.0.1[:port]`, and MCP's `allowed_hosts` are the same. So the local API is reachable only on the same machine or through a port forward that keeps a loopback host (for example `ssh -L`). The public viewer listener (`PublicViewer::router`) serves the same read routes (`viewer_routes`) for public projects only, with optional password (`/api/v1/viewer/auth`) and optional live updates.
- **Node memory.** `NodeMemory` lives inside storage behind `Memory` (`crates/storage/src/memory.rs`, `pub(crate)`, an `Arc` shared by `Storage` clones). It is a cache, not an authority: reads still probe the database identity. The viewer must read through `Storage`, never around it. `Storage::memory_stats()` is public.
- **SQLite data root is single-process.** `Store::open` creates the root and takes an exclusive lock on `<root>/.instance.lock`; a second opener fails with `DATA_ROOT_LOCKED` ("Another backend owns this data root"). With a shared `~/.fagent`, a running `fagent-server` and the desktop app in local SQLite mode **cannot run at the same time**, in either order. Inside one process a second `Store::open` of the same root fails too.
- **PostgreSQL is multi-process and initializes on open.** `PostgresBackend::with_config` runs `schema::initialize` (advisory lock, `CREATE SCHEMA` / `CREATE TABLE IF NOT EXISTS`). A probe must not call it. Several servers may share one database; there is no owner lock.
- **Live updates are in-process only.** Event streams (`events.rs`) replay from the durable commit log but are *woken* only by this process's `state.events` broadcast. Commits made by another process (another server on the same PostgreSQL database) do not wake them.
- **Identifying a FAgent server.** MCP `initialize` reports `serverInfo` `FAgent` / `0.1.0` (`mcp.rs`); `GET /api/v1/viewer` returns `{readOnly, liveUpdates, overscan}` with no product name.
- **Engine.** `engine-wasm` builds natively except `src/web/` (~1,150 lines: wasm-bindgen API, WebGPU/WebGL2 canvas, backend fallback). `Model` (`load_snapshot(json)`, `frame_keys`, `compute`, `reuse`, `export_record`/`import_record`, `hit_test`, `bounds`) and `draw` are target-neutral. `paint-vello`'s `gpu` feature and `wgpu` are enabled only for `wasm32` today, `wgpu` only with `webgpu`.
- **Rejected list.** `docs/REJECTED-APPROACHES.md` rejects `vello_gpu` **for server export** (determinism, readback cost). Drawing to a native window is a different use, not covered by that entry; export stays on `vello_cpu`. Record the distinction in `docs/DESKTOP.md`.
- **UI.** The web viewer UI is ~14,500 lines of TypeScript/React (`web/src`); none of it is reusable in a native UI.

## Deployment cases

Two binaries:

1. **`fagent-server`** (existing): headless server. Behavior, flags and env unchanged.
2. **`fagent-desktop`** (new): viewer app; the server inside it is optional.

The app has two connection modes, chosen app-wide in settings:

- **Local**: the app opens storage itself (in-process `Storage`); the MCP listener is optional.
- **External server**: the app opens no storage and no listener; it reads another FAgent server over HTTP + SSE. "External" means any other process, including one on the same machine (`http://127.0.0.1:<port>`).

| # | Case | Mode | MCP listener | Database | Viewer data path |
| --- | --- | --- | --- | --- | --- |
| C0 | First run (default) | Local | **off** | SQLite `~/.fagent` | in-process `Storage` |
| C1 | All local | Local | on, `127.0.0.1:<port>` | SQLite `~/.fagent` | in-process `Storage` |
| C2 | External server | External | — | — (the server owns it) | HTTP + SSE |
| C3 | Server only | — | — | — | use `fagent-server`; or C1 with the window hidden in the tray |
| C4 | Local server, external database | Local | on | PostgreSQL | in-process `Storage` |
| C5 | All local, port taken | Local | on, user-chosen port | SQLite `~/.fagent` | in-process `Storage` |

The in-process listener always binds `127.0.0.1` only; serving other machines is out of scope (other machines use their own app in External mode through a port forward or the public viewer).

**Shared `~/.fagent` (user decision).** Both binaries default to `FAGENT_DATA_ROOT`, else `~/.fagent`. Because of the SQLite lock:

- App start in Local + SQLite when the root is locked (a `fagent-server` owns it): show "This data root is used by another FAgent server", probe the configured port and `3847` for a FAgent server, and offer **Connect to it** (switch to External with that URL) or **Choose another data root / database**. Never retry in a loop, never touch the lock file.
- While the app holds the root (even with MCP off), `fagent-server` on the same root fails with `DATA_ROOT_LOCKED`. `docs/DESKTOP.md` and the app's MCP setting explain that the app's own MCP toggle replaces running `fagent-server` on that root.
- C5 therefore covers a port held by an unrelated process, or by a FAgent server on another root or on PostgreSQL. A FAgent server on the same SQLite root is caught earlier by the lock.

## Design

### Server embedding API (`crates/server`)

- **Storage options:** move `StorageOptions` into the library as a typed config built without env: `Sqlite { data_root }` or `Postgres { config, data_root }`, with `open() -> Result<Storage>` (unchanged behavior). The env/flag parsing stays in `startup`.
- **Probes (read-only, bounded by the configured timeouts, never write):**
  - SQLite: does not create the root or the lock file. Reports `Missing` (will be created on Apply), `Available` (with project count, read-only), `Locked` (another process owns it), `InUseByThisApp` (the root the app has open: answer from the live `Storage`, never a second open), or an error.
  - PostgreSQL: connects with `PostgresConfig` timeouts and runs read-only queries only (connectivity, server version, whether the runtime schema and its registry exist, project count). Reports `Ready`, `Uninitialized` (Apply will create the schema), or the error message. Never calls `PostgresBackend::with_config`.
- **Listener:** `ServerHandle::start(state: AppState, bind: SocketAddr) -> Result<ServerHandle>` serves `router(state)` on a runtime the caller owns; `stop()` calls `begin_shutdown()` and drains with the existing grace period; `local_addr()`.
- **Every MCP change restarts `AppState` (user decision).** Local mode always has one `AppState`. Turning MCP on or off, or changing its port, stops the old listener, stops or drains the old `Jobs` runner (add the missing stop; in-flight jobs finish or fail cleanly), then builds a new `AppState` with the new `Config` over the **same** `Storage` clone (no storage reopen, node memory kept) and starts the listener when MCP is on. Switching to External mode drops the `AppState` and the `Storage`; switching back to Local opens both again. In-process subscribers move to the new `events` channel.
- **Port probe:** `port_probe(port) -> PortStatus`: `Free` (a bind on `127.0.0.1:<port>` succeeds and is released), `InUse { fagent: Option<FagentInfo> }`, or `Denied`. To identify a FAgent server, add an additive field `server: {name: "FAgent", version}` to `GET /api/v1/viewer` (both listeners) and document it; fall back to the response shape for older servers. Naming the owning process (pid/name) is best effort per OS and optional. Never signal or kill it.
- `main.rs` uses the same API; `fagent-server --help` output and every env/flag stays identical; existing startup tests pass unchanged.

### Viewer data sources (`crates/desktop`)

One trait, two implementations, so the UI does not know which mode it is in:

- **`LocalSource`:** holds the app's `Storage` and the current `AppState`. Snapshot (with the same ETag/revision semantics as `snapshot_http`), scoped reads, fonts, assets and change events come from the same functions the HTTP handlers call, exposed from the server crate as a small `embedded` module; no HTTP, no SSE. Events: subscribe to the in-process `events` broadcast and replay from the durable commit log, the same pattern as `events.rs`. After an `AppState` restart each tab resubscribes and replays from its last revision, so no commit is missed. With MCP off nothing in the process writes. Other processes are not watched: SQLite is single-process by its lock, and a PostgreSQL database is managed by the user (see below).
- **`RemoteSource`:** base URL (+ optional public-viewer password) over the documented HTTP API and SSE (`docs/CONTRACT.md`), same payloads and ETag handling as `web/src/snapshotHttp.ts`. Works against a local listener (loopback URL) and a public viewer origin; handles `PUBLIC_LIVE_DISABLED` (no live updates: show "reload for latest").

**Memory sharing, as it really is.** `LocalSource` reads come from the one `NodeMemory`: no second storage-side copy, no HTTP/JSON transport. Each open tab's engine `Model` still holds its accepted snapshot (it takes JSON today). Phase 1 keeps `load_snapshot(json)`; phase 6 measures whether handing the engine a typed, shared snapshot is worth it.

### Native viewer and tabs

- **Window:** `winit` + `wgpu` native (Metal / Vulkan / DX12), drawing through `paint-vello` (`gpu`) with `vello_cpu` fallback, reusing `engine-wasm`'s `Model` and `draw`.
- **UI toolkit:** decided by the phase 0 spike (candidates: `egui` via `eframe`/`egui-wgpu`, `iced`, `slint`). Criteria: embeds a custom `wgpu` paint surface, tabs, IME/text input for settings, accessibility, binary size, RAM.
- **Tabs (user decision):** one window with several tabs; each tab is one project view with its own engine `Model` and UI state (page, camera, selection, inspector, scroll). Opening a project that already has a tab focuses that tab (one tab per project in this task; two views of one project is a follow-up). Background tabs keep their `Model` and state but do not draw. Switching tabs draws without recompute when nothing changed. Changing the connection mode or the database closes all tabs after a confirmation.
- **First UI scope (not parity with `web/`):** project list, tabs, page/frame board with pan/zoom, frame selection and hit test, node inspector (read-only props), live updates, bottom bar. Layers panel, notes, interaction states, motion and exports are follow-ups listed in `docs/DESKTOP.md`.

### Tray: hide, don't quit (user decision)

- Closing the window **hides** it, whatever the MCP state: the process keeps running in the tray/menu bar and the app is "frozen" as it was. Reopening shows exactly the same tabs, pages, cameras, selections and inspector state.
- While hidden the app may release the swapchain/surface textures, but keeps every tab's `Model`, frames and UI state. Change events are still consumed; changed frames are recomputed when the window is shown again (only the changed ones), so the data is current and the UI state is unchanged.
- Quit (tray menu, app menu, `Cmd+Q`) stops the listener with the existing grace period and closes storage. The last session (open tabs with project id, page, camera, selection) is saved to the config dir on Quit and restored on next launch (setting, default on); a tab whose project no longer exists is dropped with a notice.
- Tray menu: Open window, status line (mode, MCP `Off` / `On 127.0.0.1:<port>`), Copy MCP URL, Turn MCP on/off, Settings, Quit.
- Single instance: a second launch focuses the running one (lock in the config dir or a platform single-instance crate).
- macOS: `Accessory` activation policy while the window is hidden. Launch at login: optional setting, default off.

### Settings and bottom bar

Settings persist in the OS config dir (for example `~/Library/Application Support/FAgent/desktop.json`), **not** in the data root. A PostgreSQL URL with a password and a public-viewer password go to the OS keychain, never into the JSON file or logs. For tests, an env override (for example `FAGENT_DESKTOP_CONFIG_DIR`) points the config dir at a temporary directory.

Settings screen:

- **Connection mode:** `Local` (default) or `External server`.
- **Local → MCP server:** toggle (default off); host fixed to `127.0.0.1`; port (default 3847). **Test port** shows `Free`, `In use by FAgent <version>` (with **Connect to it** → External mode with that URL), `In use by another process` (process name when the OS allows) or `Denied`.
- **Local → Database:** `SQLite` (data root, default `FAGENT_DATA_ROOT` else `~/.fagent`, folder picker) or `PostgreSQL` (URL, schema, pool size; the `postgres_env.rs` timeouts as advanced fields). **Test connection** runs the read-only probe and shows the result above.
- **External server:** base URL and optional password; **Test connection** calls `/api/v1/viewer` (+ `/api/v1/viewer/auth` when required) and shows name/version, read-only, live updates. The Database and MCP sections are hidden in this mode. A note explains that a local FAgent API accepts only loopback hosts: reach another machine with a port forward to a `127.0.0.1` URL or through its public viewer. A plain `http://` URL that is neither loopback nor private shows a warning.
- **Apply:** validates first, then: MCP toggle or port → restart `AppState` over the same `Storage` (tabs stay open); database or mode → close tabs, drop the old `AppState` and `Storage` (releasing the SQLite lock), open the new one. Any failure keeps the previous working config running and shows the error.
- **PostgreSQL is user-managed (user decision).** The user owns the database, its access and its other clients. Apply opens it exactly as `fagent-server` does (`PostgresBackend::with_config`, which initializes the runtime schema when missing); only Test connection is read-only. The app does not coordinate with, or watch for, other servers writing to the same database: their commits show up when a tab reloads, not live. `docs/DESKTOP.md` states this.

Bottom bar (always visible): mode (`Local` / `External <host:port>`), database (`SQLite` / `PostgreSQL <host>`, never credentials), MCP (`Off` / `On 127.0.0.1:<port>` with copy / `Error: …`), render backend in use (`wgpu Metal`, `vello_cpu`, …). Clicking an item opens its settings section.

## Work

0. **Spike (decision gate, ≤ 2 days).** Native `winit` + `wgpu` window drawing one snapshot through `Model` + `paint-vello` `gpu`, embedded in each candidate toolkit at least as far as "custom paint surface + tabs + one text field". Measure idle RAM, a 229-frame fixture board draw, binary size. Confirm `vello_gpu` 0.4 with native `wgpu` backends builds and matches `vello_cpu` within the viewer's existing tolerance. Write results, the toolkit choice and every new dependency (tray, keychain, config dir, single instance) in `docs/DESKTOP.md` after checking `docs/REJECTED-APPROACHES.md`. Stop and report to the user if `vello_gpu` does not work natively.
1. **Server embedding API:** library `StorageOptions`, probes, `ServerHandle`, `Jobs` stop, port probe, `/api/v1/viewer` `server` field; `main.rs` on top of it. Tests use temporary data roots; PostgreSQL tests `#[ignore]` like the existing ones.
2. **Engine native host:** the target-neutral host boundary in `engine-wasm` (or the split crate), native surface + fallback in `crates/desktop`. The wasm build behaves identically.
3. **Data sources:** `LocalSource` and `RemoteSource` behind one trait, with the same observable results for the same project (snapshot revision, frames, fonts, events).
4. **App shell:** window, tabs, viewer UI scope, settings, bottom bar, tray hide/restore, session restore, single instance, settings persistence and keychain, the locked-root flow.
5. **Docs:** `docs/DESKTOP.md` (modes and cases C0–C5, shared root and lock behavior, settings, security notes, build/run), architecture diagram, README section.
6. **Measure:** idle and loaded RAM for C0/C1/C2 with 1 and 5 tabs, against `fagent-server` + browser viewer on the same fixture; hidden-window RAM; whether a typed shared snapshot for the engine is worth it (report numbers, implement only if the user agrees).

## Acceptance

### Acceptance Criteria

- `fagent-server` unchanged: same `--help`, flags and env behavior; existing server and startup tests pass. `/api/v1/viewer` only gains the additive `server` field.
- No test reads or writes the real `~/.fagent` or the real config dir: tests set the data root and `FAGENT_DESKTOP_CONFIG_DIR` to temporary directories.
- **C0:** first run opens a window on local SQLite, MCP off, no TCP listener open (checked), bottom bar shows `Local · SQLite · MCP Off`.
- **C1:** turning MCP on restarts `AppState` and starts the listener on the chosen port without reopening storage (same `Storage`, node memory loads unchanged, open tabs kept); an agent `create_project` + batch through `/mcp` appears in the open tab through the in-process event path (no HTTP request from the viewer, checked by a request counter or log); turning MCP off closes the port and the viewer keeps working.
- **Shared memory:** in C1 the process has exactly one `Storage`; `memory_stats()` shows one load per open project whether the first read came from MCP or the viewer.
- **C2:** External mode against a `fagent-server` started by the test on another temporary root (`http://127.0.0.1:<port>`) shows the same projects/frames and live updates; also against a public viewer listener with a password and with live updates off (the "reload for latest" state). No local storage or listener is opened.
- **Shared root lock:** with a test `fagent-server` holding the root, the app in Local + SQLite shows the locked-root flow and **Connect to it** switches to External; with the app holding the root, a `fagent-server` start on it fails with `DATA_ROOT_LOCKED`.
- **C4:** PostgreSQL through settings (`#[ignore]` test with a temporary database). Test connection reports `Ready` / `Uninitialized` / errors correctly, and creates no schema, table or project (checked in the catalog); Apply on an empty database initializes the schema as `fagent-server` does and the viewer works.
- **C5:** with the port held by another listener, Test port reports `In use` (and `FAgent <version>` when it is a FAgent server); saving another port rebuilds `AppState` on the same `Storage` (node memory loads unchanged) and the listener answers there; **Connect to it** switches to C2.
- **Tabs and tray:** two projects in two tabs keep their own camera/selection; closing the window keeps the listener answering and the process alive; reopening shows the same tabs, cameras and selection with 0 `compute` when nothing changed, and only the changed frames recomputed after an MCP edit while hidden; Quit stops the listener within the grace period; relaunch restores the saved tabs; a second launch focuses the first instance.
- **Secrets:** no database or viewer password in the settings file, session file, logs or bottom bar (test greps the written files and captured logs).
- **Wasm viewer unchanged:** `web` scripts `test:engine`, `test:engine-board` (already `--strict`) and the `web` unit tests pass.
- `cargo clippy`, `cargo fmt --check`, `scripts/check-public-content.sh` pass; any file over 1000 lines explained.

### Verify

```sh
cargo test -p fagent-server
cargo test -p fagent-storage
cargo test -p fagent-desktop
cargo build -p fagent-desktop --release
npm --prefix web run test:engine
npm --prefix web run test:engine-board
scripts/check-public-content.sh
```

Manual on macOS (record in `docs/DESKTOP.md`): C0 → C1 → C5 → C2 by hand, the locked-root flow with a real `fagent-server`, tray hide/restore with several tabs, launch at login off by default. Windows/Linux: build and a C0 smoke if a machine is available; otherwise list as not verified.

## Outputs

- Branch `feat/native-desktop` with the crates above, `docs/DESKTOP.md`, updated architecture/README/CONTRACT/SERVER docs.
- `docs/agents/LOG.md` entry: assignment, spike decision, results, open items.
- Spike numbers, the toolkit decision and the new dependency list in `docs/DESKTOP.md`.

## Decisions (user, 2026-11-28)

- Closing the window hides the app to the tray and freezes its state; reopening restores it exactly. Several tabs in one window.
- No LAN binding for the in-process server. A viewer connecting to any other server, `127.0.0.1` included, is the External mode case.
- The desktop app shares `~/.fagent` with `fagent-server` (one of them at a time, enforced by the existing SQLite lock).
- Turning MCP on or off, changing its port, or switching to/from External mode restarts `AppState`.
- PostgreSQL is managed by the user: Apply opens it like `fagent-server`; no cross-process live updates.

## Open questions (do not block phases 0–3)

- Two tabs on the same project (for example two pages side by side): wanted later?
- Should `fagent-server`'s `DATA_ROOT_LOCKED` message mention the desktop app (a server-side text change, outside this task's owned scope)?
