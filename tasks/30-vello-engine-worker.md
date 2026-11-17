# Task 30 — P1 Engine Worker: one WASM module holding scene + GPU, behind the `renderer=vello` flag

**Status:** IMPLEMENTED 2026-11-17 on `feat/vello-engine` (not pushed), awaiting acceptance. Architecture, measurements
and limits: [VIEWER.md § Vello engine](../docs/VIEWER.md#vello-engine-renderervello). ACs met, except: the long task when loading a large
board is still one ~90 ms task caused by the Layers panel (one row per node, in Canvas2D too), not part of the draw path; `npm run test:e2e`
was already broken (it waits for a 1:1 canvas that the All board does not create, checked on `49a94fe`). Still open (outside P1):
tile cache (Task 31), Safari 26 / slow machines, image mipmaps at very small zoom, glyph atlas eviction stress.
Packaged 2026-11-16. Phase P1 of [VELLO-GPU-RENDERER-RESEARCH.md](../docs/VELLO-GPU-RENDERER-RESEARCH.md).
**Depends on:** Task 29 concluding go.
**Owned scope:** new crate `crates/engine-wasm/` (merges `scene-wasm` + `paint-vello`, wasm-bindgen), new module
`web/src/engine/` (Worker bridge, camera, fallback), a new overlay in `web/src/components/` (thin canvas for
selection/hover/label), edits to `CanvasView.tsx`, `CanvasFrame.tsx`, the scene/paint hooks when the flag is on, `scripts/app.mjs` +
`scripts/build-*-wasm.mjs`, `web/vite.config.ts`, new tests in `web/tests/`, `docs/VIEWER.md`.
Do not edit server/storage, `renderer/` (kept for Canvas2D), `web/src/preview/`.
**Workspace:** new branch from `feat/vello-renderer` after Task 29 is merged into it.

## Context

Today: the Worker receives `JSON.stringify(snapshot)`, `scene-wasm` returns a scene JSON string, the main thread runs `JSON.parse`,
`indexScene`, Konva `Shape.sceneFunc` → `paintScene` Canvas2D; the overlay (selection, label, skeleton) is in the same Konva layer
(`web/src/components/CanvasFrame.tsx:70-130`); hit-testing runs on the CPU over bounds (`web/src/canvasHitTest.ts`).
wgpu/WebGL on WASM is not `Send`: the device, renderer and scene must live in the same Worker.

## Goal

With `?renderer=vello` (or localStorage), the viewer computes and draws the scene entirely in one Worker on an `OffscreenCanvas`;
the main thread only sends camera/events and draws the overlay. The default stays Canvas2D.

## Work to do

1. `engine-wasm`: keep the snapshot parsed once per revision, font bytes, images (`createImageBitmap` → texture), and the scene
   computed per frame in Rust memory (no scene JSON returned to the main thread). API: `load_snapshot`, `set_camera`,
   `render`, `hit_test(x,y)`, `bounds(ids)`, `frame_status`.
2. Backend: WebGPU by default; no `navigator.gpu` in the Worker, no adapter/device available, or a probe error →
   WebGL2; WebGL2 also failing → `vello_cpu` to `ImageBitmap`; an init error → fall back to Canvas2D, show a diagnostic, never a blank
   screen. WebGPU device lost → rebuild once, on a second failure fall back to WebGL2 for the session. Record the backend in use in the
   diagnostic; the query `?renderer=webgl2` forces the fallback for testing.
3. A Canvas2D/SVG overlay above the GPU canvas: selection, hover, label, skeleton, error; changing selection sends nothing
   to the GPU.
4. Hit-test and bounds for Inspector/Layers come from the engine, keeping the existing semantics (clip ancestor, current scene only).
5. Build: `.wasm` into `web/dist`, named by hash; Docker build like scene-wasm.

## Acceptance

### Acceptance Criteria

- Click selection, hover, Layers, Inspector, export work like Canvas2D (existing tests pass with both flags).
- No long task > 50 ms on the main thread while panning/zooming/loading (PerformanceObserver in the test).
- `renderer=vello` pixels match the Task 29 results for the same scene (same tolerance).
- Fallback: forcing a GPU error → `vello_cpu`; forcing a WASM error → Canvas2D; both have tests.
- `npm --prefix web run typecheck`, `npm --prefix web test`, clippy/fmt clean.

### How to verify

```bash
npm --prefix web test
```

```bash
npm run test:e2e
```

### Output

Code + tests; `.wasm` and draw-time measurements in `docs/VIEWER.md`; log in `docs/agents/LOG.md`.

## Constraints

Do not delete Canvas2D/Konva (Task 34 decides). Do not touch the real runtime or `~/.fagent`. Do not push.
