# Task 42 — Engine persistent scene cache (fast warm reload)

**Priority (user, 2026-11-27):** next viewer work after PR #8 (Tasks 36/37 partial), before Task 36 items 3–4, Task 37 items 3–4 and Tasks 31/35/33.
**Status:** DONE (2026-11-27), committed on `perf/viewer-large-project` (PR #8), not pushed; items 1–6 including item 5 (font window 14% of warm `synced`). Warm All pages `synced` 560 ms against 2,212 ms cold (−75%), `synced` − `backend` 123 ms, 0 `frame_keys`/`compute`, 229/229 imported, 2 draws; 904 frames warm 870 ms (`synced` − `backend` 396 ms); `--edit 1` 1/1/228; cold within run spread. Table and rerun commands: `docs/VIEWER.md` "Measurements — 2026-11-27 (engine scene cache, Task 42)". Packaged 2026-11-27 from a read-only survey of `perf/viewer-large-project` at `66d82f4`.
**Depends on:** `perf/viewer-large-project` (PR #8, not merged): paced draws, held `frame` messages, `MessageChannel` yields, the shape plan cache and the `test:engine-load` benchmark. Takes over the engine-path parts of [Task 32](32-viewer-fast-load.md) items 4–5 and its "durable scene cache for the engine path" note.
**Owned scope:**
- Rust: `crates/engine-wasm/src/model.rs`, `frame.rs`, `lib.rs` (wasm-bindgen surface), `model/tests.rs`.
- Web: `web/src/engine/frameLoader.ts`, `workerRuntime.ts` (only the sync/attach ordering the cache needs), `engine.worker.ts` (dependency wiring), `engineApi.ts`, `protocol.ts`, `useEngineScenes.ts` (pass the snapshot ETag), a new store module under `web/src/engine/` (for example `sceneCache.ts`, split if it grows); `web/src/hooks/useSnapshot.ts` only to expose the accepted ETag, read-only.
- Tests: `web/tests/engine-runtime.test.ts`, `engine-frames.test.ts`, a new `web/tests/engine-cache.test.ts`, `web/tests/engine-load.mjs` (warm mode).
- Docs: `docs/VIEWER.md` (section "Vello engine" + a measurements subsection), this file, `docs/agents/LOG.md`.
- Not owned: server, storage, the Canvas2D path (`web/src/scene/*`, `scene.worker.ts`; reading `browserSceneCache.ts`/`sceneStore.ts` for patterns is fine, changing their behavior is not), layout/scene-wasm compute results, the startup Suspense delay (Task 36 item 3), WebGPU initialization.
**Workspace:** a new branch `perf/engine-scene-cache` from `perf/viewer-large-project` (keep PR #8 reviewable); rebase onto `main` once PR #8 merges. Do not push or open a PR unless the user asks.
**PR scope (2026-11-27):** at the user's request the work was committed on `perf/viewer-large-project` and pushed to PR #8, which therefore covers [Task 36](36-viewer-cold-load-quick-wins.md) items 1, 2, 5, [Task 37](37-text-shape-plan-paint-caches.md) item 2 and this task (owned scope above, plus the code review fixes recorded in `docs/agents/LOG.md`). Deferred: Task 35 (`frame_keys` on a changed ETag), Safari/Firefox measurements.

## Context

User report (2026-11-27): after PR #8 a ~225-frame project's All pages board still takes ~6 s on a loaded machine (14 s before PR #8), and projects are expected to grow 3–4×. The user wants the first open to be the slow one and every later open to be very fast. A Chrome Performance recording on the user's machine showed ~3.8 s before the progress counter left 0/225; the user suspects WebGPU initialization (2–3 s) and decided to **leave initialization out of scope for now and focus on caching**.

Where a cold load spends its time (`docs/VIEWER.md` "Measurements — 2026-11-27", synthetic 229-frame fixture, Apple M5, new build, All pages, p50): `synced` 2,345 ms; Worker created and `/snapshot` requested ~350 ms; `backend` attached ~860 ms; worker `frame_keys` 388 ms; `compute` 1,066 ms; `render` 286 ms over 28 draws; `required_fonts` 70 ms. `frame_keys`, `compute` and the load-time draws (~75% of `synced`) grow with the frame count and are repeated in full on every reload.

What is cached today (checked against the code):

- Engine module: hash-named, `immutable` for a year (`docs/VIEWER-CACHE-POLICY.md`). Fine.
- Snapshot: the ETag lives only in RAM (`web/src/snapshotHttp.ts`, `cache: "no-store"`; the server sends `private, no-cache`), so every reload downloads and parses it again.
- Fonts and images: plain `fetch` plus a SHA-256 check on every load (`renderer/resource-bytes.ts`).
- Computed scenes: `engine.reuse()` keeps unchanged frames only inside one Worker's memory. The Canvas2D path has a durable IndexedDB scene cache keyed by content digest (`web/src/scene/browserSceneCache.ts`, `sceneStore.ts`: `fagent.browser-scenes`, 256 entries / 64 MiB / 8 MiB per scene, 350 ms deadlines); **the default Vello engine path has none**.

Facts the design depends on:

- The digest from `frame_keys` is revision-free (`crates/scene-wasm/src/content_key.rs`): subtree, resolved tokens/aliases/themes/styles, expanded instances and overrides, used font and asset hashes, viewport/theme/state. Editing one frame changes one digest; editing a token, theme, component master or font used everywhere changes every digest that uses it.
- `Frame::new` (`crates/engine-wasm/src/frame.rs`) consumes the `Scene` into a `PaintScene` + `Summary`, so nothing serializable is kept after compute. `Model::insert_scene` exists only for harnesses: it drops the digest and rebuilds diagnostics as strings, while `compute` keeps the `serde_json::Value` diagnostics from `compute_with_diagnostics_resolved`.
- `Model::bind_paint_font` silently skips a font whose bytes are not loaded: an imported scene must have every font in `scene.fonts` loaded first, or its text disappears without an error.
- The whole-snapshot resolve is lazy (`Accepted::resolved`, a `OnceCell`): if every frame comes from the cache and `frame_keys` is skipped, it never runs.
- `Scene` serialization uses tagged and untagged enums (`crates/layout/src/model/*.rs`); `Cargo.lock` has no binary codec today. JSON through the existing `serde_json` is the safe first format.
- Frames wait for the backend: the loader's `yieldTask` in `workerRuntime.ts` loops `while (attaching) await deps.pollTask()` (test "frames wait for the backend"). Compute must keep waiting; cache reads are I/O and can overlap the attach.
- `web/tests/engine-load.mjs` opens a new browser context per run, so it only measures cold loads today; it already takes `--frames`/`--pages`.

## Goal

Reopening a project whose frames are unchanged runs no `frame_keys` and no `compute`, imports every frame from disk and draws once or twice, so `synced` is bounded mainly by backend attach. A partial edit recomputes only the frames whose digest changed. The cold path is no slower than today.

## Work

1. **Warm benchmark first.** `engine-load.mjs --warm`: one persistent profile per series (`launchPersistentContext` in a temporary directory), one cold navigation that fills the cache, then N reloads. Report the existing marks plus cache hits/misses, bytes read, IndexedDB read time, and counts of `frame_keys`/`compute`/`import`. `--warm --edit <k>` changes k frames through MCP between loads (partial invalidation); `--warm --edit-shared` changes a token used by every frame. Record today's baseline (warm equals cold).
2. **Engine API (Rust).**
   - After `compute`, make the staged frame's persistent record available to the worker, e.g. `take_record(frame_id) -> Vec<u8>`: format version, the `Scene`, its diagnostics and digest. Serialize before `Frame::new` consumes the scene, or keep what is needed; do not keep a second copy of every scene in RAM once the worker has taken it.
   - `import_record(bytes, revision, frame_id, width, height, digest) -> Summary` stages exactly what `compute` would stage, restamped to the accepted revision (as `reuse` does). Reject a record whose format version, project, frame, viewport or digest does not match. Refuse (clear error) when a font in `scene.fonts` is not loaded, instead of drawing without text; expose the record's font list so the loader can fetch fonts before importing.
   - Format: `serde_json` first. A binary codec only with measurements showing it matters and a round-trip test over every scene node kind; a new dependency needs a line here and a check of `docs/REJECTED-APPROACHES.md`.
   - Update the `model.rs` module comment ("never serialized back to the page" stays true: records go to the worker's store, not the page).
3. **Store** (`web/src/engine/`, opened in the Worker). IndexedDB database `fagent.engine-scenes`, separate from `fagent.browser-scenes`.
   - Scene records keyed by `[build, projectId, digest, width, height, theme]`, value: bytes, fonts `[{id, hash}]`, assets, size, last use. Never key by revision.
   - Digest manifests keyed by `[build, projectId, etag, framesKey]` → digest per frame id, so a load with the same strong snapshot ETag skips `frame_keys`.
   - `build` = the engine module's content hash from `engine-wasm/manifest.json` + the record format version. No hash (dev module) → persistence off.
   - Bounds sized for ~1,000 frames from measured record sizes (state the numbers; the Canvas2D 256-entry cap is too small); least-recently-used eviction on write; other builds evicted first. `navigator.storage.persist()` best effort.
   - Hard deadlines on open and transactions (as `sceneStore.ts`); timeout, quota, corruption or a blocked database fall back to compute. The cache never fails a frame.
   - One batched read per sync (all known digests in one readonly transaction), started as soon as the digests are known so it overlaps backend attach. Writes are batched, off the critical path (idle, after the draw that shows the frames).
4. **Loader integration** (`frameLoader.ts`, `protocol.ts`, `useEngineScenes.ts`).
   - `sync` gains an optional strong `etag` from the accepted snapshot; no ETag → no manifest skip.
   - Digests: manifest hit → no `frame_keys`; miss → `frame_keys`, then write the manifest.
   - Per frame: in-memory `reuse` first (unchanged); then a cache hit → fetch missing fonts through the existing hash-checked path, `import_record`, images as today, commit; a miss → compute as today, then queue the record write.
   - Retry (`bypass`) skips cache reads and replaces the entries it recomputes. An import error deletes the entry and computes the frame.
   - Keep paced progressive draws, held `frame` messages, camera/layout ordering, a new sync superseding the old, failed-font and missing-asset semantics.
   - Backend attach: cache reads may run during attach; whether imports may also run during attach is a measured decision. Compute still waits; keep the test "frames wait for the backend", or amend it with the reason if imports move earlier.
5. **Fonts (conditional).** A worker Cache API keyed by font hash for verified bytes, only if the warm benchmark shows font fetching ≥ 5% of warm `synced`; otherwise record the number and skip.
6. Measure cold vs warm vs partial with item 1 and write the table in `docs/VIEWER.md`.

## Acceptance

### Acceptance Criteria

- `engine-load.mjs --warm` runs with the temporary fixture (no external services); generated output stays in ignored `artifacts/`; rerun commands are in `docs/VIEWER.md`.
- Warm reload of an unchanged project (default 229-frame fixture, All pages, p50 of 5 warm loads, cold measured in the same session and machine):
  - 0 `frame_keys`, 0 `compute`, every frame imported (hits = frame count), ≤ 3 draws while loading.
  - `synced` minus `backend` ≤ 300 ms (cold today ≈ 1.5 s), and warm `synced` at least 50% below cold `synced`.
- A 4× fixture (`--frames 900 --pages 40`): record cold and warm; warm `synced` minus `backend` ≤ 1 s.
- Partial (`--edit 1`): exactly one `compute`, one `frame_keys`, every other frame imported. Shared edit (`--edit-shared`): every frame recomputes and `synced` is within 10% of cold.
- Correctness:
  - Rust: an imported frame equals the computed one in summary (except revision), hit tests, bounds and `vello_cpu` pixels of a board draw (byte-identical); mismatched format, project, frame, viewport or digest is rejected; a missing font is an error, not a silent skip.
  - Web: a different engine build misses; a corrupt record is recomputed and removed; quota/timeout falls back to compute; Retry bypasses; a newer sync cancels pending imports; nothing is read from the cache before the sync carries a snapshot the server answered in this session.
- No regression: existing `engine-runtime.test.ts`, `engine-frames.test.ts` and `app-shell-smoke.mjs` expectations hold (any amended test states why); `test:engine` and `test:engine-board --strict` pass; cold `synced` not worse than today by more than run spread.
- `npm --prefix web run typecheck` clean; `cargo fmt` and clippy clean for `fagent-engine-wasm`.

### How to verify

```bash
npm --prefix web test && npm --prefix web run test:react && npm --prefix web run typecheck
cargo test -p fagent-engine-wasm
```

```bash
npm run build:engine-wasm && npm --prefix web run test:engine
REPAINT_SERVER_BIN=target/release/fagent-server npm --prefix web run test:engine-board
```

```bash
export REPAINT_SERVER_BIN=target/release/fagent-server
node web/tests/engine-load.mjs --channel chrome --scopes all,pg-1 --runs 5 --warm --label warm
node web/tests/engine-load.mjs --channel chrome --scopes all --runs 5 --warm --frames 900 --pages 40 --label warm-4x
node web/tests/engine-load.mjs --channel chrome --scopes all --runs 5 --warm --edit 1 --label warm-edit
node web/tests/engine-load.mjs --channel chrome --scopes all --runs 5 --warm --edit-shared --label warm-shared
```

Record the machine model, load average and Chrome version next to the numbers.

### Output

Code + tests; the warm benchmark; cold/warm/partial table and a short "Engine scene cache" description (keys, bounds, invalidation, failure behavior) in `docs/VIEWER.md`; status line here; entry in `docs/agents/LOG.md`.

## Security and privacy

- The cache never decides access: it is read only for a sync that carries digests or an ETag from a server response accepted in this session (after `ViewerGate`). Drawing from disk before the server answers (stale-while-revalidate) is Task 32 item 2, out of scope.
- Store only computed scene records and digest manifests: no credentials, tokens or snapshot JSON.
- Tests use temporary profiles and data roots; never `~/.fagent` or the real runtime.

## Out of scope / follow-ups

- WebGPU initialization time (the user's ~3.8 s before the first frame): measure the `backend` mark on a long-lived real Chrome profile before deciding anything.
- Showing the last drawn viewport image instantly while the engine starts (hides boot + attach): a separate task.
- Snapshot in IndexedDB with stale-while-revalidate (Task 32 item 2), the Suspense delay (Task 36 item 3), parallel compute workers and frame-scoped resolve (Task 35) for the cold path.

## Constraints

Do not change compute results, digests or pixels. Do not change the server, storage or the Canvas2D path. Do not connect to the DB of another running instance; do not touch `~/.fagent` or the real runtime. English only and public-content rules (`scripts/check-public-content.sh`). Do not push.
