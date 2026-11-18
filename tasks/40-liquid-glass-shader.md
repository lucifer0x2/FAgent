# Task 40 — Backdrop blur and liquid glass material (custom shader pass)

**Priority (2026-11-18):** backlog, not placed in the order 34 → 36 → 37 → 31/35 → 33. Requested by the user to get a
glass material close to Apple's Liquid Glass.
**Status:** Not assigned. Phase 0 (spike) needs no approval; phases 1–3 each need the user's go-ahead after the previous
phase's report.
**Depends on:** Task 30 (Vello viewer) and Task 34 (`vello_cpu` export). Touches the same tile margins as Task 31; if 31
lands first, glass must respect its tile margin rules.
**Expected owned scope:** `crates/scene-input` (props catalog, validation), `crates/layout` (model, scene builder),
`crates/paint-vello` (new glass module, encode order, CPU pass), `crates/engine-wasm/src/web/` (WebGPU/WebGL2 pass,
offscreen targets, texture cache), the scene-3 contract in `docs/CONTRACT.md`, `docs/VIEWER.md`, `docs/SERVER.md`,
`spikes/native-paint-parity/` and `spikes/vello-gpu/` (fixtures, parity). Do not touch storage, node memory or the MCP
transport beyond the new props.
**Workspace:** new branch from `main`.

## Context

Checked against the code of 2026-11-18:

- Visual props today: `fill` (solid, linear, radial; alpha allowed), `opacity`, `radius`, `clip`, `stroke` (solid colors
  only) and `shadow` (0..8 CSS box-shadow layers, `inset` supported) — `crates/scene-input/src/props_catalog.rs:132-137`.
  No blend modes, no layer filters, nothing that reads what is painted behind a node.
- Shadows are the only blur: `crates/paint-vello/src/shadow.rs` pushes a Gaussian filter layer over the node's own shape.
  A filter layer blurs its own contents, not the backdrop.
- `vello_common` 0.4 declares `DisplacementMap`, `ColorMatrix`, `SpecularLighting`, `Blend`, `Image` filter primitives,
  but `vello_cpu` 0.4 and `vello_gpu` 0.4 only execute `Flood`, `GaussianBlur`, `Offset` and `DropShadow`
  (`vello_cpu/src/filter/`, `vello_gpu/src/filter.rs`). Refraction and color filters therefore need our own pass.
- The WebGPU backend owns `device`, `queue` and the command encoder, renders a `vello_gpu::Scene` into any
  `RenderTarget` view (`crates/engine-wasm/src/web/webgpu.rs:195-232`) and already binds caller textures as images via
  `ImageSource::external_texture` (`webgpu.rs:157-192`). This is the hook for a custom WGSL pass.
- Paint order per node is outer shadow → fill → content → inset shadow → children (`crates/paint-vello/src/encode.rs`
  `contents`). The glass pass must slot in before the node's fill.
- Export runs either on Chromium/Canvas2D (`renderer/`, default backend) or on `vello_cpu`
  (`FAGENT_RENDER_BACKEND=vello`). The viewer keeps `?renderer=canvas2d`. SVG export is built in Rust
  (`crates/server/src/jobs/export_svg.rs`).
- `docs/REJECTED-APPROACHES.md` has no entry about custom shaders or post-processing passes; re-read it before adding any
  dependency.

## Target look

A glass node shows a blurred, slightly saturated copy of what lies behind it, bent like a lens near its rounded edges
(stronger toward the rim, optional per-channel offset for chromatic fringe), with a specular rim lit from a light angle,
a Fresnel-like brighter edge, and an optional tint. Children (labels, icons) draw sharp on top. Motion response (hover,
press, drag) is phase 3.

## Proposed model (final names decided at phase 1)

- `backdrop`: `{blur: 0..200 (dimension token ok), saturate?: 0..4 = 1, brightness?: 0..4 = 1}` on box kinds.
- `glass`: `{refraction: 0..1, edge: px width of the bent rim, chroma?: 0..1, specular?: 0..1, lightAngle?: deg,
  tint?: color}` on box kinds; implies a default `backdrop` when absent.
- Backdrop definition (same as CSS `backdrop-filter`): everything already painted in the same root frame before this
  node, inside the node's bounds plus the sampling margin (blur reach + maximum refraction offset), under the ancestors'
  clips. A glass node inside another glass node sees the outer result. Cap nesting (proposal: 2) and count (proposal:
  32 glass nodes per root frame); over the cap, the extra nodes render without the effect and validation reports an
  advisory.
- Not in `states`/`overrides`/`animations` until phase 3.

## Approach

Per glass node, in paint order:

1. Render the backdrop region (prefix of the scene, culled to the region) into an offscreen target.
2. Run the glass pass on it: separable or downsampled Gaussian blur; color matrix (saturate/brightness/tint); SDF of
   the rounded rect gives distance to the edge and the normal; displace the sample coordinate along the normal with a
   profile that grows toward the rim; optional per-channel offset; add specular rim from `dot(normal, light)` and a
   Fresnel term.
3. Bind the result as an image and draw it clipped to the node shape, then continue with the node's own fill (as tint),
   strokes, shadows and children.

Backends:

- **WebGPU:** WGSL compute or fragment pass on `wgpu` textures; result via `external_texture`.
- **WebGL2:** GLSL ES 3.0 port of the same pass, or blur + color only (decide in phase 0 from measured cost).
- **`vello_cpu`:** the same math in Rust on premultiplied RGBA pixmaps, sharing one parameter/profile module with the
  WGSL source so the two cannot drift.
- **Chromium/Canvas2D:** proposal: no glass; draw the node as authored (fill/stroke/shadow) and emit an advisory. Any
  other choice needs the user's decision.
- **SVG export:** proposal: the glass region is embedded as a raster `<image>` at the export scale; alternative is to
  drop the effect with an advisory. Decide in phase 1.

Open questions for phase 0:

- Segmented rendering (render the main scene up to the glass node, copy the region, continue into the same target with a
  load op that keeps contents) versus re-encoding a culled prefix scene per glass node. Measure both.
- Cache key for the backdrop texture (revision, node, device scale, region) and invalidation under pan/zoom and live
  updates.
- Interaction with Task 31 tiles: a tile needs a margin of blur reach + refraction offset, or glass nodes are drawn in a
  separate pass after tiles are stitched.

## Phases

0. **Spike (no product props).** Hard-coded glass node in `spikes/vello-gpu/`: WebGPU pass, CPU reference, timing on
   a large project (Task 24 benchmark) with 1, 8 and 32 glass nodes. Report the chosen rendering strategy and WebGL2
   decision.
1. **`backdrop` prop.** Schema, validation, contract, viewer (WebGPU, WebGL2), `vello_cpu` export, Canvas2D/SVG
   behavior as decided, design-context CSS mapping (`backdrop-filter: blur() saturate() brightness()`).
2. **`glass` prop.** Refraction, chroma, specular, tint on every Vello backend with parity tests; design context
   reports that refraction has no CSS equivalent (advisory for Task 23 conformance).
3. **States and motion.** `glass`/`backdrop` in `states`, `overrides` and animation keyframes; hover/press response.

## Expected AC

- Phase 0: a written comparison of the two rendering strategies with frame times; a recommendation the user approves
  before phase 1.
- Phase 1–2: `vello_cpu` PNG vs `vello_gpu` capture of each glass fixture stays within the Task 29 parity rule (ratio of
  pixels off by > 8/255), per fixture group (blur only, blur + saturate, refraction, chroma, specular, nested, clipped
  ancestor, glass over image, glass over text).
- A node without `backdrop`/`glass` renders bit-identical to before (existing parity fixtures unchanged).
- Viewer frame time with 32 glass nodes on the large benchmark project stays within a budget agreed in phase 0; over the
  caps, nodes degrade with an advisory, never an error or a blank frame.
- Device loss and WebGL2 fallback keep working (`simulate` faults in `webgpu.rs`).
- Docs updated: `docs/CONTRACT.md` (props, backdrop definition, caps), `docs/VIEWER.md` (pass, cache, limits),
  `docs/SERVER.md` (export behavior per backend), props catalog text and aliases.

## How to verify

- `cargo test -p fagent-paint-vello` and `cargo test -p fagent-scene-input` (new unit tests for validation, SDF/profile
  math and CPU pass).
- `node spikes/vello-gpu/parity.mjs --gpu --engine` with the new fixtures added to
  `spikes/native-paint-parity/fixtures.mjs`.
- Manual check in the viewer on WebGPU and with `?renderer=webgl2`, with screenshots attached to the report (not committed
  if generated; see `spikes/realtime-db/evidence/README.md`).
- `scripts/check-public-content.sh` before finishing.

## Outputs

`docs/TASK40-REPORT.md` with measurements, chosen strategy, parity table and open risks; one dated entry per phase in
`docs/agents/LOG.md`.
