# Export on vello_cpu (Task 34)

This records the Task 34 move of PNG/SVG export onto `vello_cpu`: the checks that were run, the parity, determinism
and timing measurements, and the runtime-dependency inventory. The commands to re-run them are in [Re-run](#re-run).

## Task 34 — export `vello_cpu`

Date **2026-11-17**, branch `feat/vello-export` (from `main` @ `228fc83`). Phases 1–3 of
[Task 34](../tasks/34-vello-cpu-export-unify.md); phase 4 was done in part following the user's decision (section [Phase 4](#phase-4--done-per-the-users-decision-2026-11-17)). The tables below were measured while the `rust` (tiny-skia) backend still existed; it has since been removed.
Measurement machine: Apple M-series, 10 CPUs / 24 GiB, **under background load** (1-minute load 4–12 because other sessions were building in parallel): timings are relative comparisons under the same conditions, not an idle-machine baseline.

**Done (phase 1).** New crate `crates/paint-core` holds the raster-independent parts that `fagent-render` used to keep:
scene wire format, validation, strict resource preparation (hashing, shared image decode with EXIF + ICC→sRGB + CMYK JPEG,
readiness, `MISSING_GLYPH`, color-font rejection), scene tree index, SVG resource checks and PNG encoding. `fagent-render`
re-exports the whole API (old tests unchanged). `fagent-paint-vello` has `export::render` with the same signature and
semantics as `fagent_render::render`, drawing with the viewer's encoder using fixed export options (glyph outlines, filter
shadows, no greek, no cull) and `vello_cpu`; the P0 decode in paint-vello was dropped. The server has `RENDER_BACKEND=vello`
(compute = `native::compute`, PNG = `vello_cpu`, SVG unchanged; SVG readiness goes through paint-core so tiny-skia is no
longer needed); separate cache identity and `engineVersion`. Operational details: [SERVER.md](SERVER.md#render-backends-render_backend).

### Checks run

| Check | Result |
| --- | --- |
| `cargo test -p fagent-paint-core -p fagent-render -p fagent-paint-vello -p fagent-server -- --test-threads=2` (rust) | 432 pass, 1 pre-existing fail `native_layout_parity` (missing `inter.ttf`) |
| Same `fagent-server` suite with `FAGENT_TEST_NATIVE_BACKEND=vello` | 379 pass, same pre-existing fail; every native test (strict resource, readiness, error codes, cancel/timeout, multiple scales, retention, color SVG, binary without Node) ran on `vello` |
| `cargo test -p fagent-paint-vello --features export` | 16 pass; includes contract comparison against tiny-skia (error codes, readiness, diagnostics, sizes, line counts) on 10 cases, cooperative cancel at every checkpoint, byte-identical PNG |
| `native-paint-parity` failure probes | 7/7 rejected, same codes as tiny-skia; still differs from Chrome on a wrong hash (`PIPELINE_MISMATCH` vs `RESOURCE_NOT_READY`) as before |
| `cargo fmt --check`, `cargo clippy -D warnings` (paint-core, render, paint-vello `--all-targets`, server lib/bin) | clean |

### Parity by feature group

The `spikes/native-paint-parity` gate is unchanged (straight RGBA, a pixel is off when any channel differs by > 8, must be
< 0.5% of all pixels). Each cell: PASS case count · largest % of pixels > 8 on straight RGBA · the same measure after
compositing over white. The Inter case is explicitly skipped (`--without-inter`, the G2 font is not in the repo), leaving
83 cases; the image group has an extra EXIF orientation 6 case that the P0 table lacks (26 cases instead of 20).

| Group | Cases | vello ↔ Chrome | tiny-skia ↔ Chrome (same run) | vello ↔ tiny-skia | P0 `vello_cpu` ↔ Chrome | vello ↔ viewer GPU (max WebGPU, WebGL2) | P0 GPU vs cpu |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Solid, odd scales | 7 | 7/7 · 0 · 0 | 7/7 · 0 · 0 | 7/7 · 0 | 7/7 · 0 · 0 | 0 | 0 |
| Gradient | 8 | 4/8 · 1.94 · 0 | 8/8 · 0 · 0 | 4/8 · 1.92 | 4/8 · 1.94 · 0 | 0 | 0 |
| Per-corner radius | 2 | 2/2 · 0.40 · 0.35 | 2/2 · 0.16 · 0.10 | 2/2 · 0.40 | 2/2 · 0.40 · 0.35 | 0.08 | 0.08 |
| Stroke inside | 4 | 4/4 · 0.39 · 0.35 | 4/4 · 0.17 · 0.09 | 4/4 · 0.36 | 4/4 · 0.39 · 0.35 | 0.09 | 0.09 |
| Stroke center | 4 | 2/4 · 0.61 · 0.52 | 4/4 · 0.28 · 0.15 | 2/4 · 0.58 | 4/8 · 0.68 · 0.59 (center + outside combined) | 0.13 | 0.13 |
| Stroke outside | 4 | 2/4 · 0.68 · 0.59 | 4/4 · 0.27 · 0.17 | 2/4 · 0.63 | (combined above) | 0.13 | 0.13 |
| Opacity + clip | 4 | 4/4 · 0.49 · 0.24 | 4/4 · 0.30 · 0.07 | 4/4 · 0.45 | 4/4 · 0.49 · 0.24 | 0.11 | 0.11 |
| Outer shadow (filter) | 8 | 0/8 · 7.05 · 0.25 | 8/8 · 0.46 · 0.07 | 0/8 · 7.05 | 0/8 · 7.05 · 0.25 | 1.47 | 1.47 |
| Inner shadow | 6 | 6/6 · 0.30 · 0.26 | 6/6 · 0.15 · 0.08 | 6/6 · 0.28 | 6/6 · 0.30 · 0.26 | 0.07 | 0.07 |
| PNG/JPEG/WebP images, crop | 26 | 13/26 · 1.80 · 1.77 | 26/26 · 0.32 · 0.31 | 12/26 · 1.86 | 8/20 · 1.80 · 1.77 | 0.09 | 0.09 |
| Text (6) + text combinations (2) | 8 | review · 6.64 · 6.06 | review · 6.65 · 6.08 | review · 2.36 | review · 6.64 · 6.06 | 1.05 | 1.05 |
| Combinations without text | 2 | 0/2 · 1.00 · 0.40 | 2/2 · 0.18 · 0.12 | 0/2 · 1.03 | 0/2 · 0.97 · 0.37 | 0.15 | 0.15 |
| **Total without text** | 75 | **44 PASS / 31 FAIL** | **75 / 0** | 43 / 32 | 39 / 30 (69 cases) | — | — |

The "vello ↔ viewer GPU" column is the % of pixels > 8 between the export PNG (`paint-vello-export`) and the `vello_gpu`
image captured in Chromium (WebGPU Metal adapter, WebGL2 ANGLE Metal), glyph outlines, filter shadows, no LOD
(`spikes/vello-gpu/parity.mjs --gpu`, the same 20 image cases as P0). It matches P0 to within 0.01 in every group: the
export path does not drift from the viewer.

**Reading the table against the AC.**

- *Against Chromium, no worse than the P0 `vello_cpu` table:* **met** in every group over the same case scope. The image
  group on exactly the 20 P0 cases is still 8/20 (GPU parity column); the new EXIF orientation 6 case passes 5/6 thanks
  to the shared decode (the P0 decode ignored EXIF). Combinations without text are 1.00 vs 0.97 because the atlas now has
  an extra EXIF image tile; on the P0 atlas (GPU parity column) it is still 0.97.
- *vello ↔ viewer outline without LOD no worse than the P0 "GPU vs cpu" column:* **met** (equal).
- *vello no worse than tiny-skia in any group:* **not met** in 6 groups: gradient, stroke center, stroke outside, outer
  shadow, images, combinations without text. Since the paint fixes (`PAINT-FIXES.md`) tiny-skia passes **75/75** cases
  without text (the 47/28 table in the [parity README](../spikes/native-paint-parity/README.md) and the Task 34 context
  are out of date). The gate was not loosened.

**Cause of each FAIL group** (all are `vello_cpu` raster, not compute or the resource contract — every case has the same
size and the same readiness/diagnostics as tiny-skia and Chrome; `drawStatsMismatches` is 0):

| Group | Cause | Visible over white? |
| --- | --- | --- |
| Gradient (alpha, radial) | Straight RGB at very low alpha: after un-premultiplying, a 1-unit premultiplied difference at alpha 1–3 becomes Δ 85–255. Vello interpolates premultiplied like CSS but its LUT quantization/rounding differs from Skia; tiny-skia emulates Skia's premultiplied stops (64 steps). | No (0%) |
| Outer shadow | Same mechanism in the Gaussian blur tail (low alpha); the `vello_cpu` filter kernel differs from the three box blurs approximating a Gaussian that Skia (and tiny-skia) use. | 0.09–0.25%, under the gate |
| Stroke center/outside @1 | AA at the edge/seam between differently colored sides at scale 1 (scale 2 passes); tiny-skia has a Chrome-style "AA seam push". | Yes (0.50–0.59%) |
| Images, crop | `ImageQuality::Medium` sampling (float bilinear) ≠ Skia's 4-bit-weight bilinear that tiny-skia emulates (`image_sample.rs`); a real difference when enlarging the 63×41 image. | Yes (0.50–1.77%) |
| Combinations without text | Sum of the groups above. | 0.30–0.40% |
| Text (review) | skrifa outline + Vello AA ≠ Chrome's CoreText glyphs; line/glyph positions match. Vello differs from tiny-skia by ≤ 2.36%. | — |

Trying the `vello_cpu` f32 pipeline (`RenderMode::OptimizeQuality`) for export: PASS count unchanged (44/31), images
worse (1.98%), so it is **not kept**.

### Determinism

| Check | Result |
| --- | --- |
| Same input, several times in one process (`export_tests::png_bytes_are_deterministic_across_runs`, scale 2.4, text + image + gradient + shadow + opacity) | Byte-identical |
| 12 board frames, 3 starts of the `vello` server (new process, new data root) | 12/12 PNGs with identical SHA-256 across starts (also true for `rust` and `chromium`) |
| Tallest frame (1440×4000) through the `paint-vello-export` adapter, 5 processes | Byte-identical to each other **and** to the PNG the server returns (`0effb8bd…`) |
| 1 thread ↔ N threads | **Could not be checked.** `vello_cpu` 0.4 is multithreaded only with the `multithreading` feature, which needs the `ordered-channel` crate that is not in the offline cache; downloading the crate needs the user's permission. Export currently runs on 1 thread (`RenderSettings::default()`, SIMD auto-detected: NEON on this machine). Other architectures/SIMD levels were not measured. |

### Export time and memory

Board `web/tests/repaint-fixture.mjs`: 12 frames 1440 × 1800–4000, 4 944 nodes (cards with shadow, images, borders, wrapped
text), through a real server job (MCP `render_frame` → PNG ready), release server, temporary SQLite, port 0. Each backend
was started 3 times; *cold* = the first 12-frame pass right after start (the first frame includes worker/Chromium startup),
*warm* = the next 2 passes with scale 1 − k·10⁻⁶ to change the job key (no cache hit) while keeping the raster size. Peak
RSS = the whole server process tree (including the Node worker and every Chromium process), sampled every 100 ms. Unit
ms; p50/p95 over all frames (cold 36 samples, warm 72 samples), board = sum of 12 frames.

Run B (less loaded machine, load 4–5; order chromium → vello):

| Backend | Server start | First frame (cold) | Cold frame p50 / p95 | Cold board p50 / max | Warm frame p50 / p95 | Warm board p50 / p95 | Peak RSS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `chromium` | 303 | 233 | 254 / 294 | 2 501 / 2 526 | 132 / 151 | 1 577 / 1 681 | 1 393 MiB |
| **`vello`** | 102 | 203 | 203 / 267 | **1 972** / 2 279 | **125** / 162 | **1 430** / 1 661 | **324 MiB** |

Run A (heavily loaded machine, load 7–12; order vello → rust → chromium):

| Backend | Server start | First frame (cold) | Cold frame p50 / p95 | Cold board p50 / max | Warm frame p50 / p95 | Warm board p50 / p95 | Peak RSS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `chromium` | 405 | 411 | 328 / 614 | 3 547 / 5 224 | 178 / 284 | 2 006 / 3 414 | 1 393 MiB |
| `vello` | 104 | 223 | 367 / 505 | 3 538 / 3 944 | 184 / 315 | 2 356 / 3 019 | 267 MiB |
| `rust` (tiny-skia) | 102 | 4 796 | 5 263 / 8 839 | 59 478 / 73 588 | 3 623 / 8 030 | 42 067 / 72 555 | 219 MiB |

`vello` breakdown on the tallest frame (1440 × 4000, 412 nodes), same scene the server computes, 5 adapter processes:
prepare + encode + `vello_cpu` raster **45–51 ms**, PNG compression (`png` 0.18 `Balanced`, 1 thread) **59–60 ms**, whole
process 111–119 ms; the server job for the same frame ≈ 125–160 ms (the rest is compute + job overhead). Trying
`Compression::Fast`: PNG 19 ms but the file is 1.77 MB instead of 450 KB (×3.9), so **not changed yet** (it would also
change the bytes of `rust`; left for the user to choose).

**Reading against the AC "vello faster than chromium cold and warm".** In run B: cold is 21% faster (board p50 1 972 vs
2 501), warm 9% faster (board p50 1 430 vs 1 577; frame p50 125 vs 132) but warm frame p95 is slightly slower (162 vs 151).
In run A (vello ran first, when the machine was most loaded): cold ties (3 538 vs 3 547), warm is slower (2 356 vs 2 006).
Conclusion: **met at p50 on a normal machine, but the warm margin is thin and load-sensitive**; the solid advantages are
startup (102 vs 303–405 ms), first frame and **4.3–5.2× less RAM**. `rust` (tiny-skia) is ~25× slower on a board with
shadows (three full-canvas box-blur passes). The next lever is not raster: PNG compression (~45% of the job), then
`vello_cpu` multithreading.

### Phase 3 — inventory of runtime Node/Chromium dependencies

Classification: **have** = a Rust equivalent already exists (usable with `rust`/`vello` now); **done here** = missing, done
in this task; **P4** = needs doing, belongs to phase 4 (awaiting approval); **dropped** = not done, with a reason.

| # | What still needs the worker / `renderer/` / Playwright / `layout-step` | Rust equivalent / decision | Class |
| --- | --- | --- | --- |
| 1 | Worker ops `compute`, `render`, `compute_render` (`worker/cli.mjs`, the Chromium branch of `jobs/runner.rs`, `Painter::Chromium`, `worker.rs`, `Config.node`) | `native::compute` + `Painter::Native{TinySkia,Vello}` | have; removed in P4 |
| 2 | Text measuring in Chromium (`renderer.measureBatch`, `worker/pipeline.mjs`) | `fagent-text` HarfRust with Chrome-style advance quantization, in `compute_with_measurer` | have |
| 3 | `layout-step` spawned on every run (`worker/pipeline.mjs:101`) | `fagent_layout::compute_with_measurer` in-process | have; removed from runtime in P4 |
| 4 | Canvas2D paint in Chromium | `vello_cpu` (this task) / tiny-skia | have (done here) |
| 5 | Identity cache runs `node` + Playwright (`fingerprint::pipeline`) | `native_pipeline` / `vello_pipeline` from compile-time input only | have (done here for `vello`) |
| 6 | Readiness, strict resource, decode observation, draw counters, diagnostics, cancel on deadline | `paint-core`, shared by both native rasters | done here |
| 7 | Error code differs from Chrome: wrong hash is Chrome `RESOURCE_NOT_READY` / native `PIPELINE_MISMATCH` | Keep the native code (more specific, recorded in the probe and SERVER.md); the other 6 probes match | dropped (with reason) |
| 8 | ICC/CMYK | `paint-core` decode: ICC RGB/gray → sRGB, CMYK JPEG via moxcms, other color spaces `UNSUPPORTED_FEATURE`; `paint-vello` previously ignored ICC/EXIF | done here; ICC parity against Chrome still lacks a licensed fixture (`spikes/native-icc-parity`) |
| 9 | Color fonts (COLR/CBDT/sbix/SVG) that Chrome can draw | Native PNG rejects explicitly with `UNSUPPORTED_FEATURE`; SVG still embeds the font (`svg_color_font`, `native_unsupported` pass on `vello`) | dropped in P4; drawing color emoji is a separate task if needed |
| 10 | System font fallback for missing glyphs (Chrome) | `.notdef` + `MISSING_GLYPH`, no implicit fallback (decision #2 above) | dropped (with reason) |
| 11 | `?scene=server` forces Canvas2D (`web/src/engine/flag.ts:24-35`) | The server scene is already computed natively when the backend is `rust`/`vello`; the Vello engine does not yet accept the server scene | P4: engine draws the server-computed scene |
| 12 | `web/` imports from `renderer/`: 26 files (`types` ×24, `index` ×6 `drawScene`/`loadResources`/`paintScene`/`indexScene`, `resource-bytes` ×4 including `engine.worker.ts`, `image-fit`, `corners`, `stroke`, `font-family`) | Move types/utilities to `web/src/scene/` before deleting `renderer/` | P4 |
| 13 | Canvas2D at the end of the Task 30 fallback chain, `?renderer=canvas2d`, Konva/`react-konva` (5 files: `CanvasView`, `CanvasFrame`, `EngineCanvasView`, `engine/overlay.ts`, `scene/yieldFrame.ts`) | The engine already has a `cpu` branch (`vello_cpu` WASM) as the last fallback; the overlay needs porting off Konva | P4 |
| 14 | `Dockerfile`: Node runtime + Playwright headless shell + `worker/` + `renderer/dist` in the image; `compose*.yaml` already default to `rust` | The runtime image needs only the server binary + `web/dist`; Node only in the web build stage | P4 |
| 15 | `scripts/native/build-release.sh` (default `chromium`, copies `node`, `worker`, `renderer`, Playwright; this task only adds `vello` to the valid values), `prepare-runtime.sh`, `run-server.sh` (inverted comment), `tests/release-smoke.sh` (`rust` branch) | A native release needs no Node/Chromium when `vello` is chosen | P4 |
| 16 | `scripts/app.mjs` builds `renderer/` and sets `PLAYWRIGHT_BROWSERS_PATH` | Drop once chromium is no longer a backend | P4 |
| 17 | Tests/harness: `test:pipeline` (`spikes/pipeline`), `test:e2e` builds `renderer/`, `web/tests/canvas2d.mjs`, server tests that run Chromium (`animation_context`, `svg_color_font`, `native_layout_parity`…), the `native-paint-parity` oracle | Chromium remains only as a **test oracle** (devDependency); server tests move to `vello` | P4 |
| 18 | PNG metadata "actual Node/Chromium versions" (`browserVersion`, `runtimeVersion` in `record.rs`) | Native writes `engine: native`, `engineVersion` per raster, `runtimeVersion: rust-native` | have |
| 19 | SVG readiness tied to `fagent_render` | `paint-core::inspect_svg_resources` | done here |
| 20 | Strict text: `TEXT_SHAPING_FAILED`, an inked glyph without an outline → `UNSUPPORTED_FEATURE` | `paint-vello` export checks each line before encoding | done here |
| 21 | Cancel: the `vello_cpu` raster is a single uninterruptible call | Cancel is checked between prepare, each text line, encode and raster; the native lane is still held until done (like `rust`) | have (limit recorded) |
| 22 | `vello_cpu` multithreading | Needs the `ordered-channel` crate (not available offline) | P4 or when the user allows downloading crates |

**Compute comparison** (phase 3.3): on the 12-frame / 4 944-node board (11/14/40 px text with wrap), the bounds and line
lists (text, x, y, width) of `rust` and `vello` match `chromium` exactly: 0 nodes with bounds drift, 0 nodes with line
drift, largest Δ 0. The Task10 corpus (`native_layout_parity`: wrap, typography, missing-glyph, long page × 4 viewports × 2
states) could not run in this worktree because the G2 `inter.ttf` is missing (pre-existing failure); its last run is the
Task10 evidence. `vello` compute is `rust` compute itself, so any remaining compute difference belongs to `rust`.

### Phase 5 — native `vello-gpu`: not doing

The condition for phase 5 was that CPU raster is a significant bottleneck; the measurements say it is not: `vello_cpu`
raster (including prepare and encode) is ≤ 51 ms on the largest frame, < 40% of the job, less than PNG compression
(60 ms). Native GPU would also add a ~23 MB readback per 1440 × 4000 frame, device initialization, and break determinism
(GPU AA/float ≠ CPU, "GPU vs cpu" column up to 1.47%) so identity/cache would have to be split by the raster actually
used. The maximum estimated gain is ≤ 40 ms/frame, smaller than changing the PNG compression level or enabling
`vello_cpu` multithreading. Not doing it; revisit if much larger frames (scale 4) show raster dominating.

### Phase 4 proposal (submitted; the decision is in the next section)

Trade-offs the user needed to settle first:

- **`vello`** = one image model with the viewer (PNG ↔ GPU viewer differ as in P0: 0 in most groups, at most 1.47% in
  shadow), faster than Chromium at p50 and ~4–5× less RAM, but **differs from Chrome in 6 groups** (gradient/shadow only
  in low-alpha straight RGB; images and stroke @1 are visible over white at 0.5–1.8% of pixels).
- **`rust`** (tiny-skia) = closest to Chrome (75/75 without text) but ~25× slower on a board with shadows and different
  from the viewer.

Proposal (separate task after approval, in order):

1. Default `RENDER_BACKEND=vello` in the server (`backend.rs`, `startup/options.rs`), `compose*.yaml`,
   `scripts/native/build-release.sh`; fix the comment in `run-server.sh`. Old caches invalidate themselves because the
   identity differs. Server tests run native on `vello` by default (`FAGENT_TEST_NATIVE_BACKEND`), keeping one `rust` pass
   if the backend is kept.
2. Allow downloading the `ordered-channel` crate to enable `vello_cpu/multithreading`; add `FAGENT_VELLO_THREADS`
   (proposed default `min(available_parallelism − 1, 8)`), verify the PNG is byte-identical 1 ↔ N threads before enabling.
3. Choose the PNG compression level for `vello`: keep `Balanced` (small file) or `Fast` (−40 ms on the large frame, file ×3.9).
4. `web/`: move `renderer/` types and utilities (item 12) to `web/src/scene/`; the engine accepts the server-computed scene
   (`?scene=server`); the last fallback is the `cpu` engine (`vello_cpu` WASM) instead of Canvas2D; port the overlay off
   Konva, then remove `react-konva`, `?renderer=canvas2d`, `web/tests/canvas2d.mjs`.
5. Remove the `chromium` backend, `worker/`, `renderer/`, `layout-step` from the runtime, `fingerprint::pipeline`,
   `Config.node`; `Dockerfile` uses Node only in the web build stage; `scripts/app.mjs` does not build `renderer/`;
   Playwright only in `devDependencies` for parity oracle/E2E; update `test:pipeline`, `test:e2e`, `docker/smoke.mjs`,
   release smoke.
6. Ask separately: keep `rust`/`crates/render` (the painter closest to Chrome, as a reference) or remove it.
   `crates/paint-core` stays in both cases.
7. Optional quality work (separate task): Skia-style 4-bit image sampling and pushing the stroke AA seam in the Vello
   encoder if PNG needs to be closer to Chrome; this would change the viewer too.

### Phase 4 — done per the user's decision (2026-11-17)

The user chose (in chat): **default `chromium` via a build variable**, the native release build on this machine uses
`vello`, **remove `rust`/tiny-skia**; removing `chromium`, `worker/`, `renderer/`, Konva or Canvas2D was not requested
(kept as is). The differences of `vello` from Chrome were accepted (text up to ~6.6%, 44/75 cases without text).
Phase 5 skipped (section above).

- `FAGENT_RENDER_BACKEND=chromium|vello` (default `chromium`) is read at build time by `scripts/app.mjs`
  (`npm run build`, `build:release`), `scripts/native/build-release.sh` and the `Dockerfile` (ARG, passed by
  `compose.yaml`); the server embeds it as the default (`RenderBackend::BUILD_DEFAULT`, `option_env!`), and
  `RENDER_BACKEND` / `--render-backend` still override it. `vello` does not build `renderer/dist` or `layout-step` and
  does not package `worker/`, Playwright or a browser; the `vello` image uses `debian:trixie-slim`, no Node/Chromium.
  Running `chromium` without the worker runtime stops at startup with a message naming
  `RENDER_BACKEND`/`FAGENT_RENDER_BACKEND`. Compose drops the `rust` default and passes `RENDER_BACKEND` only when set;
  the healthcheck uses bash.
- `deploy.sh` builds the release with `FAGENT_RENDER_BACKEND=vello`.
- Removed `crates/render`, `RenderBackend::Rust`, `NativeRaster`; the server's native tests run on `vello`;
  ICC/EXIF decode tests and SVG resource tests moved to `crates/paint-core/tests`. The `native-paint-parity` harness
  dropped the tiny-skia adapter/column (`--native`); `native-icc-parity` uses the `vello` export adapter.

| Check (2026-11-17, worktree `feat/vello-export`) | Result |
| --- | --- |
| `cargo test --no-fail-fast -p fagent-paint-core -p fagent-paint-vello -p fagent-server -- --test-threads=2` (`FAGENT_LAYOUT_BIN` release) | 409 pass, 12 ignored, 1 pre-existing fail `native_layout_parity` (missing `inter.ttf`) |
| `cargo build`, `cargo fmt --check` | clean |
| `cargo clippy -D warnings`: paint-core + paint-vello `--all-targets` (`export`,`cli`), server `--lib --bins` | clean (server `--all-targets` has pre-existing clippy errors in files not touched) |
| `npm --prefix web run typecheck` | clean |
| `FAGENT_RENDER_BACKEND=vello npm run build:release` (no `renderer/dist`), server with a PATH without `node`, workspace with only `web/dist`, `docker/smoke.mjs` + bash healthcheck | 7/7 steps, PNG via `imageUrl`; `--help` reports default `vello`; `RENDER_BACKEND=chromium` stops at startup with the message |
| Default `npm run build:release`, `docker/smoke.mjs` + healthcheck | 7/7, default `chromium` |

Not checked: `docker build` (both ARG values; needs pulling images), a real `build-release.sh`/`deploy.sh` and
`release-smoke.sh` on a `vello` release (needs a separate PostgreSQL runtime); the k8s probe (`deploy/kubernetes`) still
uses `node`, which only suits the default `chromium` image.

### Re-run

```sh
cargo build --release --locked --offline -p fagent-layout --bin layout-step
cargo build --release --locked --offline -p fagent-server
npm run build:renderer                                   # only for the chromium backend in the bench
FAGENT_LAYOUT_BIN=$PWD/target/release/layout-step cargo test -p fagent-paint-vello -p fagent-server -- --test-threads=2
# fagent-server native tests now always run on vello (FAGENT_TEST_NATIVE_BACKEND was removed)
cargo test -p fagent-paint-vello --features export
node spikes/native-paint-parity/run.mjs --vello --without-inter        # the tiny-skia column was removed
node spikes/vello-gpu/build.mjs --profile speed          # needs wasm-bindgen 0.2.129 (WASM_BINDGEN)
node spikes/vello-gpu/parity.mjs --gpu
node spikes/vello-gpu/export-bench.mjs --backends chromium,vello --starts 3 --warm 2 --breakdown
```

Measurement output goes to `spikes/*/out/` (ignored).
