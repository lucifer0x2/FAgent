# FAgent: GPU-drawn viewer (Vello + wgpu) — research and proposal

Survey date: **2026-11-16** (P0 spike results: 2026-11-17, section "P0 results"). Repo `FAgent`, branch `feat/vello-renderer`, built on branch
`feat/node-memory-writes` @`9e17583`. Research only: no product code changed, Vello not yet built.
Complements [VIEWER-FRAME-PAINT.md](VIEWER-FRAME-PAINT.md) and [VIEWER-PROGRESSIVE-CACHE.md](VIEWER-PROGRESSIVE-CACHE.md).

## Conclusions

1. **It can be done, but use `vello_gpu`, not the `vello` crate.** From late 2026 Linebender moved the compute renderer
   (`vello` 0.12) into `research/` and calls it "experimental"; `vello_gpu` 0.4.0 (formerly `vello_hybrid`, *sparse strips*
   architecture: the CPU handles paths, the GPU rasterizes + composites) is the production GPU direction, with its **own
   WebGL2 backend** and a wgpu (WebGPU) backend. `vello_cpu` 0.4.0 shares the same image model, is the most mature, and is
   used as the fallback/export path. Graphite has also moved to `vello_gpu` + WebGL2 on the web (PR #4807, 2026-10-25).
2. **FAgent's scene fits Vello well.** The `scene-3` scene has only 4-corner rounded rects, solid/linear/radial fills,
   per-side strokes, outer/inner box shadows, group opacity, rounded clips, fit/crop images and text. No masks, blend
   modes or filters — exactly the parts `vello_gpu` still lacks (mask layers panic). No "advanced effects" are needed at this stage.
3. **The GPU alone does not solve the three problems users hit.** Measurements in the repo show load time is mostly
   network/cache/JSON/fonts, not raster (warm reload 819 ms, of which layer drawing is only 161 ms). The **data
   architecture** must change together with the renderer: a single Rust/WASM engine in a Worker holding snapshot + layout +
   scene + GPU, receiving deltas instead of whole snapshots, redrawing by dirty region. A GPU renderer is the condition for
   doing that cleanly, not the whole answer.
4. **Do not move layout/text shaping to the GPU.** Flexbox (Taffy) and shaping (HarfRust) are tree/sequential algorithms;
   moving them to compute shaders brings no benefit. The "compute the scene" part needs to be **incremental** (keep the Taffy
   tree, re-measure only dirty text), not GPU. Raster/composite is already on the GPU thanks to `vello_gpu`. To be explicit:
   this optional item is **not worth doing**.

## Current state (from the base-branch code)

| User problem | Cause in the code |
| --- | --- |
| Renders per page only | One viewport-sized Konva `Stage`/`Layer` (`web/src/components/CanvasView.tsx:121-180`); the All board only lays out the root frames of the current page (`web/src/scene/frameLayout.ts`). Changing page re-runs `frame_keys` + loads scene/resources. |
| Slow to open even with cache | The snapshot is re-downloaded whole (~2 MB at 10k nodes) on every reload because the ETag lives only in RAM (`web/src/snapshotHttp.ts:22-30`); the 3.6 MB `.wasm` is served `no-store`, downloaded and compiled again; `frame_keys` parses the whole snapshot JSON (`crates/scene-wasm/src/content_key.rs:63`); every frame from IndexedDB needs SHA-256 + `JSON.parse` (`web/src/scene/browserSceneCache.ts:76-90`); fonts go through `FontFace` + `document.fonts.ready`, images through `image.decode()` (`renderer/resources.ts:32-128`). There is no timing instrumentation in the product code. |
| Scene computed on the CPU (WASM) | `SceneComputer.compute_scene` takes/returns **JSON strings** and recomputes the whole frame: parse snapshot, `make_input`, a fresh Taffy, shape text (`crates/scene-wasm/src/engine.rs:89-115`); a frame not yet cached also parses the snapshot twice (`bindings.rs:35`). The result is `JSON.parse`d again in the Worker (`web/src/scene/workerRuntime.ts:104-106`). |
| A small edit redraws the whole frame | The SSE `commit` already carries `changedIds` (`crates/server/src/events.rs:78`) but the client ignores it and GETs the whole snapshot again (`web/src/hooks/useSnapshot.ts:29-37`); a frame with a changed digest → recompute the whole frame (`crates/layout/src/engine.rs:21-120`) → new scene object → `indexScene` again, bitmap discarded (`web/src/paint/frameBitmap.ts:58-66`) → redraw the whole Konva layer. No tiles, no dirty rect. |

Also: the Canvas2D painter draws text with `ctx.fillText(line.text)` using the browser font (`renderer/paint-text.ts:14`);
the scene holds only strings + line positions, the HarfRust glyphs are thrown away. The native painter `crates/render`
(tiny-skia) has to re-shape every line (`crates/render/src/text.rs:65-99`).

## Vello in November 2026 (checked against crates.io/upstream README)

| Crate | Version | Date | Notes |
| --- | --- | --- | --- |
| `vello_gpu` | 0.4.0 | 2026-11-11 | WebGL2 (feature `webgl`) + wgpu 30; mask layers, complex filter graphs and some non-isolated blends **panic**; glyph cache still experimental; has blurred rounded rect, blur/drop shadow filters, bicubic images, external textures, WebGL shader precompile, renderer probe. No `Scene::append`. |
| `vello_cpu` | 0.4.0 | 2026-11-11 | SIMD + multithreading, full alpha/luminance masks; "most mature". |
| `vello` (compute) | 0.12.0 | 2026-11-11 | WebGPU only (compute shaders), web "not a primary target", still has dynamic memory bugs (#366, #606). Has `Scene::append`. |
| `glifo` | 0.5.0 | 2026-11-11 | Glyphs for sparse strips (`scene.glyph_run`). |
| `wgpu` | 30.0.2 | 2026-11-12 | `SurfaceTarget::OffscreenCanvas` on the web. |
| `parley` | 0.13.0 | 2026-11-12 | Not needed: keep HarfRust because it already matches Chrome's line breaking. |

MSRV 1.89; the Vello workspace pins skrifa 0.49 — the **same** version `crates/text` uses, so sharing font bytes/skrifa
causes no version conflict.

WebGPU is on by default in: Chrome/Edge (Win/macOS/ChromeOS/Android), Safari 26, Firefox 141+ on Windows / 145+ on Apple Silicon.
Not default yet: Linux (Chromium, Firefox), Firefox Android.
**Decision (the user, 2026-11-16): WebGPU is the primary path, WebGL2 the fallback** when the browser has no WebGPU, the
WebGPU probe fails, or the device is lost and cannot be recreated. Both share `vello_gpu::Scene`, so the encoding part is
not duplicated; WebGL2 must still meet the same feature acceptance because it is the path for Linux, Firefox Android and
Firefox Workers.

Reference: Figma runs a C++/WASM renderer on WebGPU and falls back to WebGL mid-session when the device is lost (blog
2025-09-18); CanvasKit has no official WebGPU yet.

## scene-3 → Vello mapping

| Scene feature | How it is drawn | Risk |
| --- | --- | --- |
| 4-corner rounded rect (CSS clamp) | `kurbo::RoundedRect` with `RoundedRectRadii`; reuse the clamp algorithm from `renderer/corners.ts` / `crates/render`. | Low |
| Solid / linear / radial fill | `peniko` brush; premultiplied interpolation like CSS by inserting extra stops (done today in `renderer/fill.ts`). | Low |
| Per-side stroke, inside/center/outside | Ring path when the 4 sides are identical; otherwise draw 4 trapezoids (seams as in `stroke-sides.ts`). | Medium: AA seams differ from Chrome |
| Outer box-shadow | `blurred_rounded_rect` (sigma = blur/2, spread grows rect/radius), clip out the part under the shape as CSS does. | Medium |
| Inner box-shadow (inset) | Clip to the shape, then draw a blurred "hole"; `vello` 0.12 has `invert`, `vello_gpu` needs checking (or a blur filter layer). | **Needs a spike** |
| Group opacity | `push_layer` alpha (isolated). | Low |
| Rounded clip | `push_clip_layer` rounded rect. | Low |
| fit/contain/cover/fill images + crop | Image brush + transform + clip; decode with `createImageBitmap` → texture. | Low; mipmaps at small zoom need checking |
| Text | The engine keeps glyph ids/advances from HarfRust and emits `glyph_run` directly — no re-shaping. Underline = rect. | Medium: AA/hinting differs from Chrome `fillText`; "viewer = export" was already accepted in Task 12 |

Not used: masks, blend modes, general filters → avoids all the `vello_gpu` panic areas. When advanced effects are needed
later: `vello_gpu` filter layers, or a custom WGSL pass over an intermediate texture.

## Proposed architecture

```
Main thread (React UI)                         Worker "engine" (one Rust module → WASM)
───────────────────────                        ───────────────────────────────────────────────
Layers / Inspector / toolbar (DOM)             ProjectStore   snapshot in RAM + apply deltas by revision
Thin Canvas2D/SVG overlay:                     LayoutCache    Taffy tree + text measurements kept per frame, mark_dirty
  selection, hover, label, guide               SceneStore     binary scene (glyph runs, paths) + R-tree bounds
Pointer/wheel ──camera (x,y,scale)──────────▶  Compositor     tile/frame texture cache, dirty regions
            ◀──hit result, bounds, progress──  vello_gpu      wgpu(WebGPU) ▸ WebGL2 → OffscreenCanvas
                                               fallback       vello_cpu → ImageBitmap (no GPU)
```

Principles:

1. **A single owner.** wgpu/WebGL on WASM is not `Send`; the device, renderer, scene, layout and snapshot live in the same
   Worker, and the main thread talks to it only with the camera/small events. The `JSON.stringify` snapshot → Worker →
   JSON scene → `JSON.parse` → TS painter loop is removed entirely.
2. **Infinite canvas in world space.** All frames of the page are placed in one space; an R-tree of bounds is used for
   culling and hit-testing. Other pages keep their computed scene in RAM (LRU) so switching pages is instant; the next page
   is prefetched when idle.
3. **Two drawing layers.** The content layer (GPU) is redrawn only when content or zoom level changes; the overlay layer
   (Canvas2D/SVG on top) draws selection/hover/labels, so click/hover never touches the GPU.
4. **Texture cache per tile.** Content is rasterized into 512×512 device-px tiles per quantized zoom level (for example
   powers of √2). Pan = composite existing tiles (almost free); while zooming = scale the nearest-level tiles, and after
   ~150 ms of rest re-rasterize the visible tiles (the Task 12 strategy and the current frame bitmap, but generalized to
   large frames). At low zoom, a tile covers many frames but only the frames intersecting the tile are drawn.
5. **Real dirty rects.** Node delta → old bounds ∪ new bounds (including shadow/stroke/overflow as in `SceneTree.extent`)
   → only the tiles intersecting that region are re-rasterized. Editing one text node = a few tiles, not the whole frame.
6. **Incremental from server to pixel.**
   - Server: the SSE `commit` already has `changedIds`; add an endpoint/SSE payload returning the **changed rows** from
     the `node-memory` `Delta` (`crates/node-memory/src/ops/delta.rs`) by `sinceRevision`. Missing revision/gap → fall back
     to a snapshot GET as today.
   - Engine: apply rows to the ProjectStore; compute the set of affected nodes (token/style/component master propagating to
     instances — an index already in `node-memory`); Taffy `mark_dirty` along ancestors, keeping the text-measurement cache
     for unchanged nodes.
   - Result: changed node bounds/scene → dirty rect → tiles.
7. **Fast load when a cache exists.**
   - Snapshot + ETag stored in IndexedDB; opening the page shows the stored copy immediately, then `If-None-Match`/delta
     to catch up.
   - `.wasm` named by content hash, `Cache-Control: immutable`, `WebAssembly.compileStreaming`; cache the module.
   - Fonts/images: Cache API by SHA (hash already exists), fonts straight into Rust (no `FontFace`), images via
     `createImageBitmap` in the Worker.
   - Binary scene cache (postcard/bincode) instead of JSON + SHA of the whole payload; or drop the scene cache entirely
     once incremental layout is fast enough and cache only the snapshot.
   - Priority order: frames intersecting the viewport first, the rest when idle.
   - Add timing instrumentation (`performance.mark`) to the product code to know the real numbers instead of inferring.

## Roadmap

| Phase | Content | Condition to continue |
| --- | --- | --- |
| **P0 Spike** (1–2 weeks) | Crate `crates/paint-vello`: scene-3 JSON → `vello_gpu::Scene` (the whole mapping table, text from `crates/text` glyphs). WASM demo page drawing the `test:repaint` fixture of 12 frames/4 945 nodes through WebGPU (primary) and WebGL2 (fallback); native `vello_cpu` draws the same scene to PNG. | Measure: `.wasm` size (brotli), encode + render ms/frame at Fit and 100% DPR 2, difference against the Canvas2D/tiny-skia PNG; inset shadow works. Go if the whole board renders in ≤ 16 ms at Fit on an M-series chip and WASM grows by ≤ ~1.5 MB brotli. |
| **P1 Engine Worker** | Merge scene-wasm + painter into one module; OffscreenCanvas; flag `?renderer=vello` (default Canvas2D); the main thread sends the camera, receives hit/bounds; overlay split into its own layer. | Select/hover/Layers/Inspector behave as before with both flags; no long task > 50 ms on the main thread. |
| **P2 World canvas + tiles** | Whole page in world space, R-tree, tile cache per zoom, instant page switching (LRU). | Pan at 60 fps at Fit and 100% with the 10k-node fixture. |
| **P3 Fast load** | Durable snapshot/ETag, immutable wasm, Cache API fonts/images, viewport priority, timing instrumentation. | Warm reload to first pixel < 300 ms (currently 819 ms to done). |
| **P4 Incremental** | Delta endpoint from `node-memory`; ProjectStore applies deltas; Taffy dirty; dirty rect → tile. | Edit 1 text node: edit → pixel < 50 ms (currently 148 ms), only dirty tiles rasterized. |
| **P5 Unify export** (optional) | Server export moves `crates/render` to `vello_cpu` so the viewer (`vello_gpu`) and PNG share the sparse-strips model; drop Konva/`renderer/` once parity is enough. | Viewer ↔ PNG parity within the tolerance set in P0. |

Advanced effects (blur layers, blend, mask) come after P5: wait for `vello_gpu` or write a WGSL pass over an intermediate texture.

## P0 results

Measured 2026-11-17, branch `feat/vello-p0`, MacBook Apple M3 Pro, Chromium 156.0.8078.4 (Playwright 1.64.1, full build in
new headless: WebGPU adapter `apple/metal-3`, WebGL2 `ANGLE Metal`), 1440×900 CSS px, DPR 2 (canvas 2880×1800).
Code and how to re-run: [spikes/vello-gpu/README.md](../spikes/vello-gpu/README.md). Board: 12 frames / 4 944 nodes
(1 932 text, 360 images, 1 092 frames, 1 560 rects; 960 cards with clip + shadow) laid out horizontally, 19 480×4 000 px.
Fit = scale 0.143 (whole board), 100% = scale 2 at frame 2. Numbers are the p50 of 20 full redraws (p95 in parentheses).

**Built:** `crates/paint-vello` (shared encoder behind a `Painter` trait; `vello_cpu` + `vello_gpu`; viewport culling;
text: outlines from HarfRust runs or `vello_gpu`'s `glyph_run`/glyph atlas; optional text LOD; 12 unit tests), the
`paint-vello-png` CLI, a WASM WebGPU/WebGL2 demo with pan/zoom, and the parity and measurement harness. No panic in any
scene-3 feature on both WebGPU and WebGL2 (77 parity cases × 2 backends + the board).

### Board performance (ms)

| Variant | WebGPU Fit: encode / total | WebGPU 100% | WebGL2 Fit | WebGL2 100% |
| --- | --- | --- | --- | --- |
| Baseline: glyph outlines, filter shadow | 64.5 / **85.6** (88.7) | 4.1 / 14.5 (20.1) | 64.2 / 78.8 | 3.8 / 9.8 |
| Analytic shadow | 63.0 / 71.1 | 4.1 / 11.0 | 62.9 / 72.5 | 3.8 / 7.7 |
| Glyph atlas | 28.8 / 40.5 | 2.4 / 11.5 | 28.5 / 41.6 | 2.1 / 8.1 |
| Atlas + text LOD < 4 device px | 11.4 / 19.6 (25.9) | 2.4 / 12.4 | 11.2 / 19.3 | 2.1 / 8.1 |
| Atlas + LOD + analytic | 10.4 / **16.0** (16.4) | 2.4 / 8.2 | 10.1 / **15.3** | 2.1 / 6.0 |
| Baseline, culling off | — | 18.0 / 28.6 | — | 17.4 / 24.8 |

"Total" = encode + submit + wait for the GPU to finish (WebGPU: `onSubmittedWorkDone`, which has callback latency; WebGL2:
1×1 `readPixels`), i.e. the cost of one full redraw. Submit 0.3–7 ms.

- **Pan** (rAF, 150 frames, redrawn every frame): at 100% the baseline p50/p95 is about 16.7/16.8 ms (60 fps; WebGPU 5/149
  frames > 20 ms, WebGL2 0), CPU work per frame 5 ms. At Fit the baseline is only 20 fps (50/66.7 ms); the atlas + LOD +
  analytic variant holds 60 fps (p95 16.8 ms, CPU work 9.6/11.7 ms) on both backends.
- **The bottleneck is the CPU** (sparse strip generation in single-threaded WASM), not the GPU: at Fit encode is ~75% of
  the frame. Text is most of it (85 719 glyphs → 54 ms with outlines; the atlas brings it to ~18 ms; LOD to ~1 ms). Without
  text, the 4 944 nodes take ~11 ms.
- **WASM SIMD is required:** the same code without `simd128` encodes Fit in 169 ms (2.6× more). **Culling is required:**
  100% goes from 28.6 down to 14.5 ms (141/4 944 nodes drawn).
- **Initialization** (warm): WASM download + compile 25–30 ms; WebGPU adapter + device + renderer 26 ms; WebGL2 renderer +
  shader compile 35 ms (~430 ms the first time on the machine, after which Metal caches the shaders); first frame
  (pipelines + encode) 117 ms WebGPU / 103 ms WebGL2. Parsing the 2.6 MB scene JSON 20 ms; loading 2 MB of fonts (SHA-256 +
  parse) 41 ms; image decode 3–6 ms.
- **Native `vello_cpu`** (single thread, 2880×1800): Fit encode 46.5 / total 81.5 ms; 100% 3.3 / 42.4 ms; LOD +
  analytic Fit 26.2 ms. Usable for export/fallback, not for interaction.
- **WebGPU vs WebGL2:** identical pixels in every parity case; comparable timing (WebGL2's GPU wait is shorter partly
  because of how it is measured). WebGL2 is good enough as the fallback path.
- **`.wasm` size** (demo: encoder + `vello_gpu` wgpu/WebGL + glifo + text/HarfRust + serde): size profile 3.65 MB raw /
  **0.77 MB brotli**; speed profile 3.77 MB / 0.84 MB (JS glue 13 KB brotli). HarfRust/skrifa/serde overlap with scene-wasm,
  so the real increment when merged is smaller.
- Safari 26: **not run** (no Safari automation in the measurement environment).

### Parity by feature group

The gate of `spikes/native-paint-parity`: the share of pixels differing by > 8/255 on straight RGBA is < 0.5%. "White" = the
same measure after compositing over a white background (equivalent to premultiplied, what the viewer actually sees). 77
cases (the Inter case is dropped because the font is not in the repo): `vello_cpu` 39 PASS / 30 FAIL / 8 text REVIEW;
WebGPU and WebGL2 both 40 / 29 / 8.

| Group | Cases | PASS cpu / GPU | Max difference % (straight · white) | GPU vs cpu % | Cause |
| --- | --- | --- | --- | --- | --- |
| Solid, odd scale | 7 | 7 / 7 | 0 · 0 | 0 | — |
| Gradient | 8 | 4 / 4 | 1.94 · 0 | 0 | Straight RGB at very low alpha (alpha gradient, radial); over white it matches. |
| Per-corner radius | 2 | 2 / 2 | 0.40 · 0.35 | 0.08 | AA on curved edges. |
| Stroke inside | 4 | 4 / 4 | 0.39 · 0.35 | 0.09 | — |
| Stroke center / outside | 8 | 4 / 4 | 0.68 · 0.59 | 0.13 | AA at joins/seams at scale 1 (scale 2 passes). |
| Opacity + clip | 4 | 4 / 4 | 0.49 · 0.24 | 0.11 | — |
| Outer shadow (filter) | 8 | 0 / 0 | 7.05 · **0.25** | 1.47 | Straight RGB at very low alpha in the blur tail; over white only 0.25% of pixels differ by > 8. |
| Inner shadow | 6 | 6 / 6 | 0.30 · 0.26 | 0.07 | — |
| PNG/JPEG/WebP images, crop | 20 | 8 / 9 | 1.80 · 1.77 | 0.09 | Sampling filter when enlarging the 63×41 image differs from Chrome; a real difference even over white. |
| Text (6) + text combinations (2) | 8 | review | 6.64 · 6.06 | 1.05 | skrifa outline + Vello AA ≠ `fillText`; line/glyph positions are preserved. |
| Combinations without text | 2 | 0 / 0 | 0.97 · 0.37 | 0.15 | Sum of the groups above. |

The difference is on par with the tiny-skia painter (`spikes/native-paint-parity/README.md`: shadow 3.1–6.6%, PNG 1.1–1.6%, crop
1.75–1.97%). Text through the glyph atlas differs more (GPU vs cpu up to 3.98%, text cases up to 7.0%) because the atlas
rasterizes glyphs at quantized positions; acceptable for the viewer, not for export.

### Shadow: choosing the technique

- **Default: filter layer** (Gaussian blur, sigma = blur/2, on the exact shape; outer clips out the part under the shape
  when the fill is not opaque; inset = clip to the padding box + an even-odd ring around the hole, then blur). Works on
  `vello_gpu` 0.4 on both WebGPU and WebGL2, supports per-corner radii; the shadow group over white differs by ≤ 0.26%.
- **Analytic** (`blurred_rounded_rect`, `invert` for inset) applies only when all 4 corners are equal; its falloff is sharper
  than Chrome's (the multi-shadow case is 13% straight / 10% over white) but it is ~2–14 ms cheaper per frame on the
  board (960 shadows). Using it as a **LOD at low zoom** is reasonable (the difference is invisible at scale 0.14). The
  "Not yet verified" item about inset shadow is closed: both approaches work.

### Go / no-go

| Threshold | Result | Met |
| --- | --- | --- |
| Draw the whole board in ≤ 16 ms at Fit (Apple Silicon) | Faithful (outline + filter): 85.6 ms. Atlas + text LOD + analytic shadow: 16.0 ms WebGPU / 15.3 ms WebGL2 (p95 16.4 / 16.7). | Only with LOD |
| `.wasm` grows by ≤ ~1.5 MB brotli | 0.77–0.84 MB for the whole demo module | Met |
| 100% DPR 2, pan p95 | 14.5 ms full redraw; pan 60 fps p95 16.8 ms | Met |
| No WebGPU/WebGL2 panic | Every scene-3 feature, the whole board | Met |

**Recommendation: conditional GO for P1.** GPU raster is not the problem (the GPU wait after submit is 3–17 ms, even at Fit); the
risk is single-threaded CPU strip generation in WASM. Conditions for P1/P2: (1) build WASM with `simd128`, and viewport
culling is the default; (2) zoom LOD: text under ~4 device px drawn as bars, analytic shadow at low zoom; (3) `vello_gpu`'s
glyph atlas (still "experimental") goes behind a flag and the outline path is kept as the fallback; (4) P2 caches tiles so
Fit only redraws when the zoom level changes and pan only composites; (5) measure Safari 26 and machines weaker than an
M3 Pro before turning it on by default. Not met if the requirement is a faithful redraw of the whole board every frame at
Fit without LOD/cache.

## Risks

- **Unstable API:** `vello_gpu` makes breaking changes every release and was just renamed. Pin the exact version, wrap it
  behind our own `Painter` trait so the backend (`vello_cpu`, `vello` compute) can be swapped without leaking into the engine.
- **Panics instead of errors:** catching panics (`catch_unwind` is useless with `panic=abort` on WASM) → run the renderer
  probe at startup, with a fallback path to `vello_cpu`/Canvas2D; on WebGPU device loss → recreate or fall back to WebGL2
  like Figma.
- **No `Scene::append`:** sub-scenes cannot be reused; compensate with a tile/frame texture cache (needed anyway).
- **Seen in Graphite:** scenes that are too dense must be split, infinite coordinates, gradient banding (PR #4821). Check
  in P0 with a large fixture.
- **Text differs from Chrome:** skrifa outline + Vello AA ≠ `fillText`; line breaks/positions still come from HarfRust so the layout does not change.
- **Firefox:** WebGPU in a Worker is still behind a pref; WebGL2 in a Worker (OffscreenCanvas) works in every major browser.
- **WASM size:** no published number yet; measure in P0.

## Not yet verified

- Verified in P0 (see "P0 results"): inset shadow (both the blurred rounded rect `invert` and the filter layer), the CPU
  cost of strip generation in WASM (the main bottleneck), building on rustc 1.97 + `wasm32-unknown-unknown`.
- Mipmaps/image quality at very small zoom on `vello_gpu` (P0 only compared images at scale 1–2).
- Safari 26 and machines weaker than an M3 Pro; stability of the "experimental" glyph atlas when the cache is full (eviction).

## Sources

- [linebender/vello README](https://github.com/linebender/vello/blob/main/README.md),
  [vello_gpu README](https://github.com/linebender/vello/blob/main/vello_gpu/README.md),
  [releases](https://github.com/linebender/vello/releases), crates.io API (checked 2026-11-16).
- [Graphite PR #4807](https://github.com/GraphiteEditor/Graphite/pull/4807),
  [PR #4821](https://github.com/GraphiteEditor/Graphite/pull/4821).
- [WebGPU in the major browsers (web.dev)](https://web.dev/blog/webgpu-supported-major-browsers),
  [Figma rendering powered by WebGPU](https://www.figma.com/blog/figma-rendering-powered-by-webgpu/).
- Vello issues [#366](https://github.com/linebender/vello/issues/366), [#606](https://github.com/linebender/vello/issues/606),
  [#476](https://github.com/linebender/vello/issues/476).
