# Task 31 — P2 Canvas world space + tile cache + instant page switching

**Priority (2026-11-17):** do after [Task 34](34-vello-cpu-export-unify.md) (Vello export, removing Node/Chromium from the runtime), [Task 36](36-viewer-cold-load-quick-wins.md) (first load) and [Task 37](37-text-shape-plan-paint-caches.md) (shape plan/paint cache), per the user's decision. Order: 34 → 36 → 37 → 31/35 → 33.
**Status:** NOT ASSIGNED. Packaged 2026-11-16; current state updated 2026-11-17 (section below). Phase P2 of
[VELLO-GPU-RENDERER-RESEARCH.md](../docs/VELLO-GPU-RENDERER-RESEARCH.md).
**Depends on:** Task 30 (implemented, Vello is the default). Independent of Task 35 (per-frame resolve); can run in parallel.
**Owned scope:** `crates/engine-wasm/` (world, R-tree, tile compositor), `web/src/engine/`, `web/src/scene/frameLayout.ts`,
`web/src/scene/pageBoard.ts` and the related page/camera state, tests in `web/tests/`, `docs/VIEWER.md`.
**Workspace:** new branch from `main` (which includes Task 30). The "All pages" board is currently an uncommitted change in the working tree:
wait for it to be committed before branching; do not commit it on someone else's behalf.

## Context

Today the All board only places the root frames of the selected page; switching pages needs `frame_keys` + a reload.
The per-frame bitmap cache (`web/src/paint/frameBitmap.ts`) skips frames larger than the canvas and has no tiles
(`docs/VIEWER-FRAME-PAINT.md:149-154`).
(The paragraph above describes the Canvas2D path at packaging time; the current Vello path is in the section below, which wins where they differ.)

## Current state 2026-11-17

Checked against the code on `main` + the uncommitted working tree:

- **There is no tile or raster cache.** Every draw re-encodes and rasters the whole viewport: `Backend::render`
  (`crates/engine-wasm/src/web/backend.rs:396`) calls `scene.reset_and_resize` then `draw` for WebGPU/WebGL2; the CPU path builds a new
  `vello_cpu::RenderContext` each time. `draw.rs:80-136` only culls per frame (extent + backdrop) and leaves node
  culling + LOD (greek text < 4 device px, analytic shadow when scale < 0.5) to the encoder.
- **The only pixel reuse** is `web/src/engine/staleCamera.ts`: a CSS transform of the old image to match the new camera while the
  worker computes a frame; newly exposed areas stay blank, and zooming blurs.
- **"Progressive" = redraw the whole viewport after each committed frame:** `workerRuntime.ts` (`requestRender`, `yieldTask`)
  redraws the whole board between two computed frames.
- **`set_layout` drops every scene that is not placed** (`crates/engine-wasm/src/model.rs:253-264`): leaving All pages for one
  page and coming back means the dropped frames must be recomputed (the digest only saves frames still in `frames`).
- **P0/P1 measurements** (M3 Pro, Chromium 156, 1440×900 DPR 2, 12-frame / 4,945-node board): P0 full redraw at Fit was
  85.6 ms originally, 16.0 ms with atlas + LOD + analytic (WebGPU) / 15.3 ms (WebGL2); the bottleneck is the CPU (single-thread WASM sparse strips,
  encode ~75% of the frame). Engine P1: Fit encode + submit 11.8–12.0 + 2.3 ms, Fit pan p50/p95 13.3/13.8 ms; ~100%
  pan 2.1/2.8–5.6/6.4 ms. `vello_cpu` Fit total 81.5 ms native, unusable for interaction. Sources:
  `docs/VELLO-GPU-RENDERER-RESEARCH.md` "P0 results", `docs/VIEWER.md` "Measurements — 2026-11-17".
  Consequence: the board only just reaches 60 fps at 4.9k nodes; bigger projects or slower machines will drop, because cost scales
  with the number of visible nodes, not with the number of changed pixels.

**The "All pages" board (uncommitted) partly overlaps the world space goal.** `web/src/scene/pageBoard.ts`
(`fitPageBoard`), `web/src/engine/sections.ts` (colored background + page title tabs drawn on a 2D canvas under the engine image),
the page `?page=%3Aall%3A` (`ALL_PAGES` in `web/src/frameScope.ts`, Vello path only) already place every frame of every page
into a single board layout and send it to the engine via `set_layout`. This task **builds on that layout and does not create a second
world**: world = the `FramePlacement`s of `fitPageBoard` (or `fitFrameBoard` for a single page), with the spatial index and tiles built
on those board coordinates. Note: `fitPageBoard` lays out by viewport ratio (cached by `size`), so resizing the window changes the position of
every frame. Choose and state one of two options: (a) keep the world stable on resize (layout depends only on frame/page size), or
(b) treat a layout change as invalidating all tiles. Do not change tab/section background behavior beyond what tiles need.

**Recommended first slice (~1.5–2 weeks), assigned before the rest of the task:**

1. A tile compositor for GPU backends only (WebGPU/WebGL2) in `engine-wasm`: raster tiles into textures, pan/zoom
   composites textures; the `vello_cpu` backend keeps redrawing the full viewport as today.
2. Per-frame invalidation on commit: a committed (or replaced) frame dirties the tiles intersecting that frame's old ∪ new extent;
   other tiles stay as they are. No per-node dirty rect yet (Task 33).
3. Risks to handle, with tests: tile seams (AA, shadow/blur spilling over tile edges, glyph atlas — a tile raster must have
   a margin equal to the largest shadow/blur reach or clip correctly so it does not differ from a full draw); GPU memory budget
   (count texture bytes, LRU, degrade when over); device loss (tiles are lost with the device, rebuild or fall back to WebGL2 via the
   Task 30 mechanism, never a blank screen).

Page LRU, page prefetch, time-sliced raster and an R-tree for hit-testing (items 1, 3–4 below) are later parts, coordinated with All
pages.

**Note 2026-11-17 (render speed survey, not done yet):** at All pages Fit (scale 0.042) each draw encodes 22,137 nodes
and 5,373 clip/opacity layers while each frame is only ~60 device px; drawing at half resolution is still 14.0 ms vs 14.7 ms,
so the cost is per node/layer, not per pixel. Do this before or together with per-zoom-level raster (item 2): LOD by device
size — drop nodes smaller than ~0.5 device px, drop clip/opacity layers for subtrees of only a few px
(`crates/paint-vello/src/encode.rs` `draw`/`contents`, `cull.rs`). Estimated Fit encode −40–70% (estimate); medium risk: needs a separate
parity tolerance for small zoom, recorded explicitly, and must be fully off on the export path (Task 34). Each tile at a small zoom level
is also cheaper thanks to this.

## Goal

All frames of a page live in one world space with a spatial index; pan/zoom only composites existing tiles; switching pages
does not wait for recomputation.

## Work to do

1. World: frame positions from the existing board layout (`fitPageBoard` for All pages, `fitFrameBoard`/`frameLayout` for
   a single page); an R-tree (or grid) of bounds of every drawn node, used for culling and hit-testing.
2. Tile cache: 512×512 device px tiles, quantized zoom levels (powers of √2), key (page, level, tile); a GPU byte
   budget with LRU. Pan = composite tiles; zoom while dragging = scale the tiles of the nearest level; after ~150 ms of rest, re-raster the visible tiles.
   Tile raster is split per frame to stay within the frame budget (time-slice).
3. Raster priority: tiles at the viewport center → edges → a buffer ring outside the viewport when idle.
4. Page LRU: keep the parsed snapshot and computed scene of the N most recent pages; prefetch the next page when idle. Switching pages reuses
   the scene and only rasters visible tiles. This requires `set_layout` to stop dropping unplaced frames immediately
   (`model.rs:253`) and put them into a bounded LRU instead.
5. GPU/CPU memory limits with numbers and handling when exceeded (degrade, drop far tiles).

## Acceptance

### Acceptance Criteria

- 10k-node fixture (spikes/all-frames-stress): 60 fps pan (frame interval p95 ≤ 17 ms) at Fit and 100%, DPR 2.
- A frame larger than the canvas at 400% zoom is still sharp after zooming stops, and panning does not redraw vectors.
- Switching back and forth between two already opened pages: first pixel < 100 ms.
- Tile memory does not exceed the configured budget (a measuring test).
- An image stitched from tiles matches a full draw at the same camera (same parity tolerance as Task 29/30), including shadow, blur and text cut
  across tile edges, on WebGPU and WebGL2.
- Committing one frame only re-rasters the tiles intersecting that frame's old ∪ new extent (counted in a test).
- `vello_cpu` still draws the full viewport, with existing behavior and tests unchanged; device loss → tiles rebuilt, no blank screen.
- Existing tests pass; typecheck/clippy/fmt clean.

### How to verify

```bash
npm --prefix web test
```

```bash
cargo test -p fagent-engine-wasm
```

```bash
npm --prefix web run test:engine && npm --prefix web run test:engine-board
```

```bash
node spikes/all-frames-stress/run.mjs
```

### Output

Code + tests + measurements in `docs/VIEWER.md`; log in `docs/agents/LOG.md`.

## Constraints

Do not touch server/storage, the real runtime, or `~/.fagent`. Do not push.
