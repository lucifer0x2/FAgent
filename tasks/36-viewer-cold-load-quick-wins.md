# Task 36 — Viewer: faster first load (bounded progressive drawing, remove the Suspense delay, parallel font/image loading)

**Priority (2026-11-17):** do right after [Task 34](34-vello-cpu-export-unify.md), before [Task 37](37-text-shape-plan-paint-caches.md) and Task 31/35/33. Order: 34 → 36 → 37 → 31/35 → 33.
**Status:** Partly done (2026-11-26, branch `perf/viewer-large-project`, not merged): items 1, 2 (paced draws + held `frame` messages, option (b)) and 5 done; items 3 (Suspense delay) and 4 (parallel fonts/images) open. Item 2's "off-viewport commits do not dirty the board" was not implemented (pacing bounds the draws instead). `engine-load.mjs` has no `--base-url` mode yet; the test instance was not measured. Results on a synthetic 229-frame board (M3 Pro, Chrome 158, interleaved p50): All pages `synced` 8.60 s → 2.82 s, worker `render` 4,805 ms / 413 draws → 393 ms / 33 draws (AC ≤ 350 ms not met: 33 draws × ~12 ms), one page 1.74 s → 1.20 s; table in `docs/VIEWER.md` "Measurements — 2026-11-26". Review follow-up 2026-11-27 (Apple M5, `docs/VIEWER.md` "Measurements — 2026-11-27"): commit → `drawn`, now timed from the worker's commit, p95 101–110 ms (AC ≤ 250 ms met); the `/snapshot` request mark is recorded (~350 ms, item 3 open); a camera received while a frame fetched its fonts was drawn after that frame computed (also on base) and is now drawn first; a failed last draw no longer posts held frames and `synced` without an image. Packaged 2026-11-17 from the Vello render speed survey (measured on the test instance, see Context).
**PR scope (2026-11-27):** at the user's request one branch and one pull request (PR #8, `perf/viewer-large-project`) carry this task's items 1, 2 and 5 together with [Task 37](37-text-shape-plan-paint-caches.md) item 2 (`crates/text`) and [Task 42](42-engine-persistent-scene-cache.md) (engine records in `crates/engine-wasm`, `recordStore.ts`, `sceneCache.ts`, `fontCache.ts`, `useSnapshot.ts`): the large-board work was measured as one change. Each task's own changes stay inside its owned scope; the Rust and cache files in the PR belong to Tasks 37 and 42, and this task's own scope is unchanged ("does not touch Rust" holds for its items). Deferred, not part of PR #8: items 3 and 4, `--base-url`, and the test instance measurement.
**Depends on:** Task 30 (implemented). No technical dependency on Task 34; the order is a prioritization decision. Does not touch Rust; every item can be done on the current `main`.
**Owned scope:** `web/src/engine/workerRuntime.ts`, `frameLoader.ts`, `engine.worker.ts`, `client.ts` and `useEngineScenes.ts` (only the part receiving `frame`/`drawn` and the skeleton), `web/src/main.tsx`, `web/src/app/RouteBoundary.tsx`, `web/src/auth/ViewerGate.tsx` (+ `useViewerSession.ts` if needed), tests `web/tests/engine-runtime.test.ts`, `engine-frames.test.ts`, `app-shell-smoke.mjs`, a new measurement script `web/tests/engine-load.mjs` (+ fixture), `web/package.json` (add the measurement script), the measurement section in `docs/VIEWER.md`. Do not edit `crates/`, server, storage, or the Canvas2D path/`scene.worker.ts`.
**Workspace:** new branch from `main` after the changes currently open in `web/src` and `crates/engine-wasm` (from another session) have been committed; do not commit on others' behalf, do not stash/reset other people's changes.

## Context

Read-only survey 2026-11-17, checked against the code. Measurement environment: the approved test instance (port 3900, production dist), project with 7,411 authored nodes / 21 pages / 190 root frames / 1,895 instances / 42 fonts, 1.84 MB JSON snapshot; full Chromium headless (`channel: "chromium"`, WebGPU), 1440×900 CSS, DPR 2, Layers panel closed, M3 Pro. Each number is a single run unless a range is given. Method: an init script wraps `Worker` (timestamp of each message in/out, `postMessage` cost), a `PerformanceObserver` for long tasks, `performance.getEntriesByType("resource")`; on the worker side, Playwright `route` edits the wasm-bindgen JS glue to wrap every `Engine.prototype` method with `performance.now()`, read back through `worker.evaluate`; CPU profile via Chromium trace (`disabled-by-default-v8.cpu_profiler`; `.wasm` keeps the name section so Rust function names are visible).

Cold load (marks counted from navigation):

| | pg-m6 (19 frames) | All pages (190 frames) |
| --- | --- | --- |
| Engine Worker created | 344–373 ms | 330–357 ms |
| `sync` sent (with snapshot) | 364–414 ms | 354–397 ms |
| First frame committed | 1,037 ms | 1,060–1,304 ms |
| `synced` (all frames) | 1,233–1,259 ms | 3,965–4,785 ms |
| `render` while loading (total / count) | 42 ms / 20 | **1,347 ms / 191** |
| Font loading (sequential, localhost) | 33 ms | 70 ms (33 fonts) |

Causes within this task's scope:

1. **Whole-board redraw after every committed frame.** `frameLoader.ts:215` calls `requestRender()` on each commit; `workerRuntime.ts:86-93` (`yieldTask`) draws immediately `if (dirty) render()` between two frames; after `sync` (`workerRuntime.ts`, branch `case "sync"`) it draws once more at the end. All pages: 191 whole-board draws = 1,347 ms over a 3,934 ms span (34%). An existing test pins this behavior: `engine-runtime.test.ts` "a busy worker draws each committed frame and a new layout without animation frames".
2. **~300 ms Suspense delay at startup.** `main.tsx:9-11` `lazy(App)` sits inside the `<Suspense>` of `RouteBoundary.tsx:25`. The App chunk finishes loading in ~50 ms (starting ~33 ms), but React 19.3.1 has already committed the fallback "Loading viewer…", so it holds the content until `globalMostRecentFallbackTime + 300` (`web/node_modules/react-dom/cjs/react-dom-client.production.js:12185`). Trace: the main thread is idle during this window; `/api/v1/viewer`, `/projects`, `/snapshot` only start at ~348–354 ms. This costs ~230–300 ms on every load, every page, every renderer.
3. **Fonts and images load sequentially.** `frameLoader.ts:162-179` does `await fetchBytes` for each font, each frame; `frameLoader.ts:190-205` loads + decodes each image after `compute`. On localhost 33 fonts ≈ 63–70 ms; with a 50 ms RTT about ~1.6 s (estimate, not measured).
4. **Yielding between frames with `setTimeout(0)`** (`engine.worker.ts:57`): on average ~1.1 ms/frame (206 ms over 188 frames, including parsing/posting the summary); `MessageChannel` saves at most ~150 ms on All pages.

Constraints already in the code (the original survey proposed preloading App as soon as `main.tsx` runs and fetching the snapshot in parallel with the permission check — **this must not be done**): `RouteBoundary.tsx:3` says "Chunk loading stays behind ViewerGate"; `app-shell-smoke.mjs` (the section "canvas/preview route split and authentication gate") requires that a locked viewer does not load `App.tsx`/`PreviewApp.tsx` and does not call `/api/v1/projects`, and that the preview route does not load `App.tsx`; the section "route chunk has accessible pending/error status and retry recovers" requires that a slow chunk still shows the status "Loading viewer…" and a chunk error has a "Try again" button. Any early loading must start **after** `/api/v1/viewer/auth` returns open.

Out of scope (already covered by other tasks): resolving the whole snapshot in `frame_keys` (553–664 ms, Task 35), snapshot/wasm/font caches (Task 32), tiles (Task 31), HarfRust shape plan and paint (Task 37). WebGPU attach takes ~480 ms (`backend` ~898 ms on pg-m6) and frames wait for the backend by design (`engine-runtime.test.ts` "frames wait for the backend"): this task only records that mark in the measurement script and does not change the rule.

## Goal

Cold load of All pages from ~4.0–4.8 s down to ≈ 2.5 s until every frame is ready, and every page ~0.3 s earlier, while frames still appear progressively, with no gap and no stuck skeleton.

## Work

1. **Measurement script before changing anything** — `web/tests/engine-load.mjs` (+ `engine-load-fixture.mjs` if needed), npm script `test:engine-load`. Production build (like `engine-board.mjs`: `buildDist`, `launch` from `engine-support.mjs`), full Chromium, 1440×900, DPR 2, Layers closed (`localStorage left-open=false`). Two data sources:
   - Default: a temporary backend (`startBackend` from `repaint-fixture.mjs`, SQLite in a temporary directory, loopback port), seeded through MCP with a multi-page project of ~7k nodes / ~190 frames (dense text, with instances, as many fonts as possible from the fonts available in the fixture; record the number of fonts).
   - `--base-url http://127.0.0.1:3900 --project <id>`: the approved test instance, GET only, never print credentials.
   Measure, per page (`--scopes all,pg-…`; `--pages` is the fixture's page count), N cold runs (default 5, a new context each run), reporting p50/min/max: the Worker creation mark, start of the `/snapshot` request, `ready`, `backend`, `sync` sent, first frame committed, first frame present in `drawn`, `synced`; the count and total time of worker `render` while loading (`status().renders` through `?engineDebug=1` or wrapping `Engine.prototype` through `route` as in the survey); the latency from commit → the first `drawn` after it (p50/p95/max); main thread long tasks; optional `--latency <ms>` (Playwright `route` delaying font/asset) to show the effect of parallel loading. Write `artifacts/engine-load/summary.json` (already ignored). Run and record the baseline before changing items 2–5.
2. **Bounded progressive drawing** (`workerRuntime.ts`, `frameLoader.ts`):
   - The first frame of a `sync` is drawn immediately. After that, at the yield point between two frames, draw only when dirty and ≥ ~100 ms since the last draw (a named constant, with a test). Camera, a new `layout`, and device loss still draw immediately as now. The last frame is always drawn before the `sync` finishes processing, without waiting for an animation frame (keep the rule in the `case "sync"` branch).
   - A frame commit entirely outside the viewport (placement from `layout` + current camera, with a margin for shadow/overflow) does not make the board dirty immediately; it is drawn on the next draw triggered for another reason, or at the latest when `sync` completes. If the extent cannot be determined with certainty, treat it as inside the viewport.
   - **The skeleton must not disappear before the pixels.** Currently the page removes the skeleton on receiving `frame` (and a reused `frame`). Choose one and document it: (a) the page keeps the frame's skeleton until the first `drawn` that arrives after that `frame` (marked by message order), or (b) the worker batches committed `frame`s and sends them right before the `drawn` that contains them. A failed frame is still reported immediately as now.
3. **Remove the startup Suspense delay** (`main.tsx`, `RouteBoundary.tsx`, `ViewerGate.tsx`): once access is open, start `import("./App")` (or `PreviewApp` depending on the route) and mount the route only when the module is ready, so React does not commit the fallback and then get throttled by 300 ms (for example: the gate waits for "auth open + module ready"; or a module promise marked fulfilled used with `use`). A slow chunk still shows the status "Loading viewer…" and a chunk error still has "Try again" (same text, same role). After mounting App early, measure again: if from "access open" to the `/snapshot` request it is still > 50 ms, it is allowed to start early the part that contains no project data (load/compile the engine module, create the Worker) as soon as access is open; do not fetch project data before `ViewerGate` opens, and do not create a second snapshot fetch path in parallel with `useSnapshot`.
4. **Parallel fonts/images** (`frameLoader.ts`): within a frame, load all missing fonts at once and then `load_font` in the order of `required_fonts`; load + decode images at the same time (`Promise.allSettled`), `missing` keeps the order of `summary.assets`. Add prefetching: after `frame_keys`, when idle, call `required_fonts` for the next few frames and start loading ahead, using a shared in-flight map keyed by `id:hash` (no duplicate loads). Keep the semantics: `failedFonts`, the error `Missing font: <id>` breaks exactly that frame, check `current()` before loading into the engine, a new sync cancels the old sync's work.
5. **Yield with `MessageChannel`** (`engine.worker.ts`): replace `setTimeout(0)`. The order must be preserved: `camera`, `layout`, `sync` messages from the page that arrive during computation are still handled before the next frame (add a browser test or a test with a simulated queue; if Chromium prioritizes the internal port and starves page messages, keep `setTimeout` and record the measurements).
6. Measure again with the script from item 1, update `docs/VIEWER.md` (section "Vello engine", before/after table by mark).

## Acceptance

### Acceptance Criteria

- The `test:engine-load` script runs with a temporary fixture (no external services needed) and with `--base-url`; measurement output lives in `artifacts/` (ignored), and the rerun commands are in `docs/VIEWER.md`.
- Timing (same machine, same project, p50 of 5 cold runs, baseline re-measured with this same script before changing):
  - All pages on the test instance, 190 frames: `synced` ≤ 2.8 s and at least 1.2 s lower than baseline (target ≈ 2.5 s).
  - Every measured page (at least `pg-m6` and All pages): the `/snapshot` request starts at least 0.2 s earlier (baseline ~350 ms); first frame committed and `synced` at least 0.25 s earlier.
  - Worker `render` while loading All pages: total ≤ 350 ms (baseline 1,347 ms); count ≤ span/100 ms + number of camera/layout changes + 2.
  - With `--latency 50`: font loading time of All pages drops by at least half compared with the baseline with the same flag.
- No regression of progressive display:
  - Frames intersecting the viewport appear progressively (not piled up until `synced`): commit → `drawn` latency p95 ≤ 250 ms, max recorded.
  - No frame loses its skeleton before its pixels are in a delivered `drawn` (new client test); no skeleton stuck after `synced` or after a scope change (the test "a reused frame repeats its summary, so a view that never received it drops the skeleton" passes).
  - The last frame is still drawn without an animation frame; a new layout is drawn immediately; a camera change during computation is drawn before the next frame. The test "a busy worker draws each committed frame and a new layout without animation frames" is **amended to the new contract** (first frame immediately, in between by time window with a fake clock, last frame always drawn), not deleted.
  - All other tests of `engine-runtime.test.ts`, `engine-frames.test.ts` pass without changing expectations (fonts loaded once, a failed font breaks the frame, a failed image goes to `missing`, a new sync replaces the old sync, a failed commit reports an error, frames wait for the backend, device loss, stale image by camera).
- `app-shell-smoke.mjs` passes: a locked viewer does not load `App.tsx`/`PreviewApp.tsx` and does not call `/api/v1/projects`; preview does not load `App.tsx`; a slow chunk shows "Loading viewer…", a chunk error has "Try again" and recovers.
- `test:engine` (WebGPU, WebGL2, vello_cpu, device loss, no GPU, broken module) and `test:engine-board --strict` pass; pan Fit/100% p95 is not worse than baseline by more than 10% (measured in the same run).
- `npm --prefix web run typecheck` is clean.

### How to verify

```bash
npm --prefix web test && npm --prefix web run test:react && npm --prefix web run typecheck
```

```bash
npm --prefix web run test:engine
REPAINT_SERVER_BIN=target/release/fagent-server npm --prefix web run test:engine-board
```

```bash
node web/tests/engine-load.mjs --scopes all,pg-0 --runs 5                  # temporary fixture
node web/tests/engine-load.mjs --base-url http://127.0.0.1:3900 --project <id> --scopes all,pg-m6 --runs 5   # --base-url not implemented yet
node web/tests/engine-load.mjs --scopes all --latency 50
```

The test instance on port 3900 (DB `fagent`) is read-only; do not print `~/Library/Application Support/FAgent-test/config`. Run before/after on the same machine, same build mode; record the machine model and the Chromium version next to the table of numbers.

### Output

Code + tests; the measurement script `web/tests/engine-load.mjs` (+ fixture) and npm script; before/after table by mark in `docs/VIEWER.md`; log in `docs/agents/LOG.md`.

This task has no optional items, to keep the scope in the web layer: wasm build/allocator flags belong to [Task 37](37-text-shape-plan-paint-caches.md) (optional phase); brotli compression of `/snapshot` and JS is noted in [Task 32](32-viewer-fast-load.md) (server/build, mainly beneficial for remote viewers).

## Constraints

Do not change Rust, server, storage or the Canvas2D path. Do not loosen the permission check: do not load route chunks or project data before `ViewerGate` opens. Do not connect to the DB of the other running instance or to ports 3847/3857/3858/3877; do not touch `~/.fagent` or the real runtime. Do not push.
