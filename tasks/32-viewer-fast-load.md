# Task 32 — P3 Fast load: durable snapshot, immutable WASM, font/image cache, timing

**Status:** NOT ASSIGNED. Packaged 2026-11-16. Phase P3 of
[VELLO-GPU-RENDERER-RESEARCH.md](../docs/VELLO-GPU-RENDERER-RESEARCH.md). The engine-path parts of items 4–5 and the "durable scene cache for the engine path" note moved to [Task 42](42-engine-persistent-scene-cache.md) (2026-11-27).
**Depends on:** Task 30 (engine path). Items 1, 2, 5 can be done independently and also benefit Canvas2D.
**Owned scope:** `web/src/snapshotHttp.ts`, `web/src/hooks/useSnapshot.ts`, new cache `web/src/cache/`,
`web/src/engine/`, `crates/server/src/viewer_static.rs` + `cache_hints.rs` (`.wasm`, font, asset headers),
`docs/VIEWER-CACHE-POLICY.md`, tests in `web/tests/` and `crates/server/tests/`.
**Workspace:** new branch from `feat/vello-renderer` after Task 30.

## Context

A warm reload of 12 frames takes 819 ms to finish, of which layer painting is 161 ms (`docs/VIEWER-FRAME-PAINT.md`). Causes:
the snapshot GET is repeated in full because the ETag lives only in RAM (`web/src/snapshotHttp.ts:22-30`); `/scene-wasm/*.wasm` is `no-store`
(3.6 MB / 688 KB brotli) and is downloaded + compiled again; `frame_keys` parses the whole snapshot; the IndexedDB scene needs SHA-256 + `JSON.parse`;
fonts go through `FontFace` + `document.fonts.ready`; font/asset routes have no Cache-Control. There is no timing instrumentation in the code.

## Goal

Reopening an already viewed project: the first pixel from durable local data, then catch up with the server in the background.

## Work to do

1. Measure: `performance.mark/measure` for each stage (snapshot, wasm, keys, scene, font/image, first pixel, done);
   shown via `?perf=1`, capturable in tests.
2. Store snapshot + ETag + revision in IndexedDB per project (size limit, integrity check); open the page and draw immediately from
   the stored copy (marked stale), then `If-None-Match`; 304 keeps it, 200 replaces it. Do not store credentials.
3. `.wasm` named by content hash, `Cache-Control: public, max-age=31536000, immutable`; `compileStreaming`;
   index.html stays `no-cache`. Check that public viewer and auth do not leak private data through shared caches.
   The engine module part (`/engine-wasm/`, 12-hex hash) was finished 2026-11-17; scene-wasm remains (stable URL, `no-store`).
4. Fonts/images: Cache API keyed by SHA-256 (the hash already exists), `private, max-age, immutable` headers for hash-based routes;
   font bytes go into the engine directly, images `createImageBitmap` in the Worker.
5. Scene cache (if still needed after Task 33): binary (postcard) instead of JSON; drop the whole-payload SHA if IndexedDB is already
   reliable enough, or replace it with a fast checksum.
6. Priority: frames intersecting the viewport are computed/drawn first; the rest when idle.

**Note 2026-11-17 (render speed survey, not done yet; measurements and method in
[Task 36](36-viewer-cold-load-quick-wins.md)):**

- **A parallel compute pool + durable scene cache for the engine path** (do after Task 35, overlaps items 5–6): a single Worker computes
  the 190 All pages frames sequentially (`compute` 1,449 ms) and cannot draw meanwhile. Use K scene-wasm workers
  (`web/src/scene/scene.worker.ts` already exists) feeding scenes into the engine via `Engine.insert_scene`
  (`crates/engine-wasm/src/model.rs`, which currently drops digest/diagnostics: they must be kept so `reuse` and the summary stay correct), and an
  IndexedDB cache keyed by digest for the engine path (Canvas2D already has `web/src/scene/browserSceneCache.ts`/`sceneStore.ts`, the engine
  does not). Estimated cold compute ÷K (K=4: ~1.45 → ~0.4 s), warm ≈ IDB read + parse (~20 ms per 2.6 MB, P0 number). Medium
  risk: memory per worker, digest consistency between worker and engine. About 1–1.5 weeks.
- **HTTP compression** (only helps remote/public viewers): `/snapshot` 1.84 MB is sent uncompressed (brotli-5 ≈ 156 KB) because the
  server has no compression layer for the API (`tower-http` does not enable the compression feature); the app JS (441 + 247 KB) is also uncompressed even though
  `viewer_static.rs` already has `precompressed_br()/precompressed_gzip()`, because the Vite build emits no `.br`/`.gz` files for `web/dist/assets`
  (only the engine module has sidecars via `scripts/package-engine-wasm.mjs`). Work: emit sidecars in the web build;
  compress `/snapshot` (and other large JSON) by `Accept-Encoding`, keeping ETag/`Vary` correct. About 0.5 day.

## Acceptance

### Acceptance Criteria

- Warm reload of the 12-frame fixture (`web/tests/all-frames-repaint.mjs`): first pixel < 300 ms, done < 500 ms, headless Chromium
  DPR 2; report before/after for each stage.
- A local snapshot older than the server: show stale then update to the correct revision; a project switching private → can no longer be read
  through the public viewer from cache.
- `.wasm` reload costs 0 bytes when the build is unchanged.
- Existing cache/HTTP tests pass; add tests for the new headers.

### How to verify

```bash
npm --prefix web test
```

```bash
cargo test -p fagent-server -- --test-threads=2
```

### Output

Code + tests + a before/after table in `docs/VIEWER-CACHE-POLICY.md`; log in `docs/agents/LOG.md`.

## Constraints

Do not touch the real runtime or `~/.fagent`. Do not push.
