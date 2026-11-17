# Task 29 — P0 Spike: draw scene-3 with `vello_gpu` on the web, measure go/no-go

**Status:** DONE 2026-11-17 on `feat/vello-p0` (not pushed): conditional GO, see the "P0 results" section of
[VELLO-GPU-RENDERER-RESEARCH.md](../docs/VELLO-GPU-RENDERER-RESEARCH.md#p0-results) and `spikes/vello-gpu/README.md`.
Packaged 2026-11-16. Phase P0 of
[VELLO-GPU-RENDERER-RESEARCH.md](../docs/VELLO-GPU-RENDERER-RESEARCH.md).
**Depends on:** none. Replaces the Task 09/12 direction (tiny-skia WASM) for the viewer; Tasks 09–13 stay as reference.
**Owned scope:** new crate `crates/paint-vello/` (lib), `spikes/vello-gpu/` (web demo + measurement harness + README),
a new member in the workspace `Cargo.toml`, the "P0 results" section in `docs/VELLO-GPU-RENDERER-RESEARCH.md`.
Do not edit `web/src/`, `renderer/`, `crates/render`, `crates/scene-wasm`, server, storage.
**Workspace:** FAgent repo, new branch `feat/vello-p0` from `feat/vello-renderer`. Do not push unless asked.

## Context

- Scene `scene-3` (`renderer/types.ts`, Rust: `fagent_layout::SceneNode`) is a pre-order node list with
  `parentId`: rect with 4 rounded corners, solid/linear/radial fill, per-side stroke (inside/center/outside), outer/inner shadow,
  group opacity, rounded-corner clip, image fit/crop, text (line strings and positions only).
- Semantic references: the Canvas2D painter `renderer/*.ts` (~1,500 lines) and the tiny-skia painter `crates/render`
  (already has radius clamp, stroke sides, shadow, text re-shape via `crates/text`).
- Existing fixtures: `spikes/native-paint-parity/fixtures.mjs` (cases per feature) and the 12-frame / 4,945-node
  scenario of `web/tests/all-frames-repaint.mjs`.
- Upstream: `vello_gpu` 0.4.0, `vello_cpu` 0.4.0, `glifo` 0.5.0, wgpu 30 (2026-11-11/12). `vello_gpu` panics on
  mask layers / complex filter graphs / some non-isolated blends — scene-3 needs none of these.
- Local machine: rustc 1.97, no `wasm32-unknown-unknown` target and no `wasm-bindgen` yet (the repo pins 0.2.129 for scene-wasm;
  install project-locally like `scripts/build-scene-wasm.mjs --install-tools`, do not install globally without asking).

## Goal

Know for certain whether `vello_gpu` draws FAgent scenes correctly and fast enough in a browser, with measurements to decide P1.

## Work to do

1. `crates/paint-vello`: `fn encode(scene: &PaintScene, res: &Resources, cam: Affine) -> vello_gpu::Scene` covering the full
   mapping table in the research document. Text: shape each line with `crates/text` (like `crates/render/src/text.rs`)
   then `glyph_run`; font bytes shared. Premultiplied gradients via extra stops like `renderer/fill.ts`.
   Put it behind a small `Painter` trait so the backend can be swapped (`vello_cpu` uses the same encode code).
2. Inset shadow: try an inverted blurred rounded rect / a blur filter layer; record how the choice was made.
3. `spikes/vello-gpu/`: a WASM demo page that loads scene JSON + fonts/images and draws via **wgpu/WebGPU** (primary) and **WebGL2** (fallback; chosen by
   query, default WebGPU), with simple pan/zoom; a native `vello_cpu` bin that draws the same scene to PNG.
4. Measurement harness (Playwright Chromium, DPR 2, 1440×900): encode ms, render ms, frame time while panning, at Fit and 100%;
   raw/brotli `.wasm` size; device init + shader compile time.
5. Parity: compare the `vello_cpu` PNG and the `vello_gpu` capture with the Canvas2D/Chromium PNG of the same case (premultiplied tolerance,
   ratio of pixels off by > 8/255), per feature group like the table in `spikes/native-paint-parity/README.md`.

## Acceptance

### Acceptance Criteria

- Every scene-3 feature draws without panic on WebGPU and WebGL2 (Chrome macOS); the main measurements come from WebGPU, WebGL2 is recorded separately; the Safari 26 result is recorded.
- The report has all numbers: `.wasm` brotli, encode/render ms for the 12-frame board at Fit and 100% DPR 2, pan frame time p95.
- A parity table per feature group; state clearly which groups deviate and why.
- A go/no-go recommendation against the thresholds: render the whole board ≤ 16 ms at Fit on Apple Silicon, `.wasm` growth ≤ ~1.5 MB brotli.
- `cargo clippy -p fagent-paint-vello -- -D warnings`, fmt clean; any file > 500 lines has a reason.

### How to verify

```bash
cargo test -p fagent-paint-vello
```

```bash
node spikes/vello-gpu/run.mjs
```

### Output

Spike code + a re-runnable README; the "P0 results" section in the research document; a decision log in `docs/agents/LOG.md`.

## Constraints

- Do not touch the real runtime (3847/3857/3858/3877, PG 2703, `~/.fagent`); use a temporary data root.
- Pin exact versions (`=0.4.0`); generated measurement PNG/JSON are ignored, not committed.
