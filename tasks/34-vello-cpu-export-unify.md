# Task 34 — Export with Vello (`vello_cpu`), remove Node/Chromium from the runtime

**Status:** PHASES 1–3 DONE; **PHASE 4 PARTLY DONE** per the user's decision of 2026-11-17 on `feat/vello-export`: the export backend is chosen at build time by `FAGENT_RENDER_BACKEND=chromium|vello` (default `chromium` in dev, `npm run build`, Docker/Compose; a `vello` build does not package the worker/renderer/Playwright/`layout-step`), the native release build on this machine uses `vello`, the `rust` backend and `crates/render` (tiny-skia) are removed. Removing `chromium`/`worker/`/`renderer/`/Konva/Canvas2D was **not** requested (kept as is, `chromium` remains the default). **Phase 5 skipped** (reason in the "Phase 5" section of the report). Measurements and results at [PURE-RUST-EXPORT.md § Task 34](../docs/PURE-RUST-EXPORT.md#task-34--export-vello_cpu). Packaged 2026-11-16; rewritten 2026-11-17 per the user's decision. **Priority: do before
Tasks 36, 37, 31, 35, 33** (order 34 → 36 → 37 → 31/35 → 33). Phase P5 of [VELLO-GPU-RENDERER-RESEARCH.md](../docs/VELLO-GPU-RENDERER-RESEARCH.md), pulled forward
because `crates/paint-vello` + `vello_cpu` already exist from Tasks 29/30.
**Depends on:** Task 30 (implemented, Vello is the default in the viewer). Does not depend on Tasks 31–33/35. Phase 4 needs explicit
user approval; phase 5 is optional.
**Owned scope:** server: `crates/server/src/backend.rs`, `startup.rs`, `startup/options.rs`, `state.rs`,
`fingerprint.rs`, `jobs/` (`runner.rs`, `painter.rs`, `record.rs`, `resource_bytes.rs`, `native*`, PNG/SVG export) and
matching tests; `crates/paint-vello` (native export path, keeping viewer behavior); `spikes/native-paint-parity/`,
`spikes/vello-gpu/` (parity/measurement harness); `Dockerfile`, `compose*.yaml`, `scripts/app.mjs`, `scripts/native/`;
`docs/SERVER.md`, `docs/PURE-RUST-EXPORT.md`, `README.md`, `docs/ACCEPTANCE.md` (only adding new evidence).
Phase 4 (after approval): also `worker/`, `renderer/`, Konva/`react-konva` and the Canvas2D path in `web/`,
`crates/render` (tiny-skia), `package.json`. Do not edit storage, node-memory, or the viewer engine beyond the phase 4 work.
**Workspace:** new branch from `main` (which includes Task 30). The working tree has uncommitted changes by someone else (All
pages board): do not commit them on their behalf; branch from an existing commit.

## Context

Checked against the code of 2026-11-17:

- Export for agents (MCP `render_frame`, `export_nodes` PNG/SVG) runs as server jobs. `RENDER_BACKEND=chromium|rust`
  (`crates/server/src/backend.rs:1-19`, `startup/options.rs:7-14`), default `chromium`; `--render-backend` beats the env.
- The `chromium` path (`crates/server/src/jobs/runner.rs:64-117`): a Node worker (`worker/cli.mjs` ops
  `compute`/`render`/`compute_render`) opens Chromium via Playwright (`worker/runtime.mjs:57`). Compute = the Rust binary
  `layout-step` (`worker/pipeline.mjs:101`) looping with text measurement in Chromium (`renderer.measureBatch`,
  `pipeline.mjs:159-161`); paint = `renderer/` Canvas2D in Chromium (`jobs/painter.rs:24-29`).
  The identity cache runs `node` + Playwright at startup (`fingerprint.rs:54-85`).
- The `rust` path (`runner.rs:72-74`, `125-...`): native compute in-process (`jobs/native.rs`, the same Rust as the
  viewer), PNG via tiny-skia `crates/render` ("experimental and not yet pixel-accepted", `docs/SERVER.md:83`;
  47 PASS / 28 FAIL non-text / 13 text REVIEW, `spikes/native-paint-parity/README.md:101`). SVG is already built in Rust on
  both paths (`jobs/export_svg.rs:1`), but SVG readiness calls `fagent_render::inspect_svg_resources`
  (`painter.rs:91`) and `resource_bytes.rs:19-20` is tied to the `fagent_render` types. Provenance and identity hardcode
  tiny-skia: `runner.rs:134` (`engineVersion`), `fingerprint.rs:7` (`NATIVE_RASTER_IDENTITY`).
- The viewer (default) computes the scene in the browser with WASM (`crates/engine-wasm`, including scene-wasm) and draws with
  `vello_gpu`. `crates/paint-vello` encodes for both `vello_gpu` and `vello_cpu` (features `cpu`/`gpu`), with a native CLI
  `paint-vello-png` (`crates/paint-vello/Cargo.toml:16-34`). P0 parity (`docs/VELLO-GPU-RENDERER-RESEARCH.md`,
  "Parity by feature group"): `vello_cpu` 39 PASS / 30 FAIL / 8 text REVIEW vs Chromium, on par with tiny-skia; GPU vs
  cpu deviates up to 1.47% of pixels (> 8/255) on shadows, ~1% on text outlines, up to 3.98% with the glyph atlas.
- Gaps of `paint-vello` vs `crates/render` for export: no ICC/CMYK (`paint-vello/src/resources.rs:159`
  "No ICC/CMYK handling in P0"; `crates/render/src/images.rs` has EXIF + ICC→sRGB); no strict/readiness semantics yet
  (`MISSING_GLYPH`, a bad resource → job error, decode observation), no draw counters, no cooperative cancellation on deadline like
  `painter.rs:30-69`. The viewer uses LOD (`greek_below_px`, analytic shadow) and an optional glyph atlas: export must turn all of that off
  (glyph outlines, shadow filter, no greek).
- Deployment: `compose.yaml:21` and `compose.postgres.yaml:25` already default to `RENDER_BACKEND=rust` but the image still installs
  Chromium (`Dockerfile:2-3`, `66-72`); `scripts/native/build-release.sh:42` defaults to `chromium` (the comment at
  `scripts/native/run-server.sh:16-17` says the opposite and must be corrected); `scripts/app.mjs:59-87` builds
  `renderer/` and points `PLAYWRIGHT_BROWSERS_PATH` at the runtime.
- Hidden dependencies on `renderer/` beyond the painter: `?scene=server` forces Canvas2D (`web/src/engine/flag.ts:24-35`);
  `web/src/preview/` (types, `image-fit`, `corners`, `stroke`, `font-family`, `loadResources`) and
  `web/src/engine/engine.worker.ts:1` (`resource-bytes`) import from `renderer/`; the end of the Task 30 fallback chain is
  Canvas2D; `npm run test:pipeline`/`spikes/pipeline` and `test:e2e` build `renderer/`.
- Task 13 (switching the default to `rust` on mac) is superseded by this task for the default change; do not do Task 13 separately.

## Goal

One imaging model for viewer and export. Final target: a runtime without Node/Chromium — native Rust compute (the existing
`rust` path), PNG via `vello_cpu` (deterministic, no GPU needed on the server), native SVG; `?scene=server` still works with
Rust compute; Playwright remains only a devDependency for E2E.

## Work to do

### Phase 1 — `vello` backend (`vello_cpu`) for frame PNG and node PNG

1. Add `RenderBackend::Vello` (`RENDER_BACKEND=vello`, `--render-backend vello`); the error message lists all values.
   Compute uses `native::compute` unchanged; `chromium`, `rust` stay unchanged behind the flag. The default is not changed yet.
2. `Painter::Vello` in `jobs/painter.rs` (the single PNG seam): encode with `paint-vello` using fixed export options
   (glyph outlines, shadow filter, `greek_below_px = 0`, no culling outside the frame viewport), raster with `vello_cpu`, straight-alpha
   PNG. `vello_cpu` multithreading runs inside the existing native lane (`native.rs` `LANE`); the thread count is configurable, with the default
   stated explicitly.
3. Same semantics as `Painter::Native`: strict resources (missing font/image, hash mismatch, decode failure → error codes like the `rust`
   path), readiness + decode observation, `MISSING_GLYPH`/`.notdef`, draw counters, diagnostics, cancellation on
   deadline. Image decoding is shared (EXIF + ICC→sRGB) with `crates/render` instead of the P0 decode of `paint-vello`: split it into a shared
   module/crate, do not duplicate.
4. Separate identity/provenance for `vello`: `fingerprint.rs` (a new raster identity), `engineVersion` following the
   `paint-vello`/`vello_cpu` version, native `runtimeVersion`. Startup with `vello` does not run `node`/Playwright.
5. Node PNG export (`export_nodes`, multiple scales) and `render_frame` both go through the new painter; SVG keeps `export_svg` but
   SVG readiness no longer requires `fagent_render` when `vello` is chosen (move the shared resource check out of the tiny-skia crate
   or keep the dependency with a stated reason).

### Phase 2 — parity and timing

1. Harness: `spikes/native-paint-parity` gets a `vello` adapter (the CLI `paint-vello-png` already speaks the adapter protocol)
   alongside `--native`; reports use the same `native-paint-parity/1` schema.
2. Parity by feature group (a table like "Parity by feature group" of P0), three pairs: `vello` PNG ↔ Chromium (measure both
   straight RGBA and over a white background), `vello` ↔ tiny-skia, `vello` PNG ↔ the viewer `vello_gpu` image on the no-LOD outline path
   (`spikes/vello-gpu/parity.mjs`). Record the cause of each deviating group.
3. Determinism: the same input → byte-identical PNG across runs and between 1 / N threads on the same machine.
4. Export time/memory cold/warm: `vello` vs `chromium` vs `rust` on the 12-frame / 4,944-node board
   (`spikes/vello-gpu/export-board.mjs`) and per frame through server jobs; p50/p95, peak RSS.

### Phase 3 — inventory of features that exist only in the Node worker

1. List every place in server/web/scripts that still needs the worker, `renderer/`, Playwright or `layout-step` at runtime (use the
   points in Context as a starting list; find more): worker ops, fingerprint, Chromium text measurement, readiness,
   differing error codes between Chromium and native (for example hash mismatch: Chrome `RESOURCE_NOT_READY`, native
   `PIPELINE_MISMATCH`), ICC/CMYK, color fonts, `?scene=server`, preview, test harness.
2. For each item: a Rust equivalent already exists / work needed / dropped with a reason. Do the "work needed" items that belong to export (for example ICC, readiness)
   in this task; the items that belong to the viewer (`?scene=server` drawing via the engine with a server-computed scene, moving the `renderer/` utilities
   that `web/src/preview/` and `engine.worker.ts` use into `web/`, replacing Canvas2D at the end of the fallback chain)
   are written as a plan for phase 4.
3. Compare compute: scenes from `rust`/`vello` against `chromium` on the existing corpus (wrap, baseline, text width),
   recording the remaining differences.

### Phase 4 — change the default and remove old paths (only when the user explicitly approves)

Present the parity/timing tables and the phase 3 inventory, and wait for the user's confirmation in chat. After approval:

1. Default `RENDER_BACKEND=vello` in the server, `compose*.yaml`, `scripts/native/build-release.sh`; fix the comment in
   `run-server.sh`. Caches under the old identity invalidate themselves (noted in `docs/SERVER.md`).
2. Remove `worker/`, `renderer/` (after moving what `web/` still uses), Konva/`react-konva` and the Canvas2D path in
   `web/` (including `?renderer=canvas2d`; `?scene=server` moves to the engine), `layout-step` from the runtime, the
   `chromium` backend; decide separately whether to keep or remove `rust`/`crates/render` (ask the user).
3. Playwright only in `devDependencies` for E2E/tests; `scripts/app.mjs` does not build `renderer/` and needs no browser
   to run the server. `Dockerfile` installs no Node runtime/Chromium; fix `docker/smoke.mjs` if needed.
4. Update `test:pipeline`, `test:e2e`, and the web tests pinned to Canvas2D (`web/tests/canvas2d.mjs`) accordingly.

### Phase 5 — optional: native headless `vello-gpu`

Only do this if the phase 2 measurements show the CPU is a significant bottleneck. `RENDER_BACKEND=vello-gpu` (a separate flag): `vello_gpu`
through native wgpu (Metal/Vulkan/DX12), rendering into a texture + readback; no adapter/device or a probe error → fall back to
`vello_cpu`, recording a diagnostic and the raster actually used in the provenance. Note on determinism: GPU and CPU differ in AA/float, so
identity/cache must be split by the raster actually used, and the parity oracle and digest count only `vello_cpu`; GPU PNGs are
not used as golden.

## Acceptance

### Acceptance Criteria

- Phase 1: with `RENDER_BACKEND=vello`, `render_frame` and `export_nodes` PNG/SVG run without starting a worker and
  without calling `node`; the existing server tests for the `rust` path pass when run with `vello` (strict resources, readiness, error codes,
  cancel/timeout, multiple scales). `chromium`/`rust` behavior does not change.
- Parity (the gate of `spikes/native-paint-parity`: ratio of pixels off by > 8/255 < 0.5%): `vello` is no worse than tiny-skia in any
  group; against Chromium it is no worse than the P0 `vello_cpu` table in any group; `vello` ↔ viewer outline no-LOD is no worse than the P0 column
  "GPU vs cpu". Every FAIL group has a recorded cause. Do not loosen the gate to get a PASS.
- Determinism: byte-identical PNG between runs and between 1 / N threads.
- Timing: `vello` is faster than `chromium` cold and warm on the 12-frame board; record the `rust` numbers and RSS alongside.
- Phase 3: a complete inventory table, with no "unclear" item left.
- Phase 4 (if approved): the Docker image and the native release run without Node/Chromium;
  `docker/smoke.mjs` and the server/web tests pass; Playwright remains only in `devDependencies`.
- `cargo fmt`, clippy clean for the edited crates.

### How to verify

```bash
cargo test -p fagent-paint-vello -p fagent-server -- --test-threads=2
```

```bash
node spikes/native-paint-parity/run.mjs --native   # add the vello adapter in phase 2
node spikes/vello-gpu/parity.mjs --gpu             # viewer vello_gpu ↔ vello_cpu
```

Run the server for trials with a temporary data root and port 0 (`--render-backend vello --port 0 --data-root <temp>`); additional
cross-checks are allowed on an approved trial instance (DB `fagent`, port 3900), without printing credentials. Generated measurement output is ignored,
only the conclusion tables and re-run commands are committed.

### Output

Code + tests; parity/timing/determinism tables in `docs/PURE-RUST-EXPORT.md`; the phase 3 inventory table; a
`RENDER_BACKEND` section in `docs/SERVER.md`; a proposal to remove the old paths presented to the user; log in `docs/agents/LOG.md`.

## Constraints

Do not change the default, and do not delete `worker/`, `renderer/`, Konva, Canvas2D or `crates/render` without the user's explicit
confirmation in chat. Do not connect to databases or services you did not start (see the local machine notes); do not touch `~/.fagent` or the real
runtime. Do not push.
