# Task 37 — Cache HarfRust shape plans and resolved paint/paths (faster compute + encode)

**Priority (2026-11-17):** do after [Task 34](34-vello-cpu-export-unify.md) and [Task 36](36-viewer-cold-load-quick-wins.md), before Task 31/35/33. Order: 34 → 36 → 37 → 31/35 → 33.
**Status:** Partly done (2026-11-26, branch `perf/viewer-large-project`, not merged): item 2 done (`crates/text/src/plans.rs`, per-font `PlanCache`, max 32 plans, shared by clones; captured corpora unchanged). Worker `compute` on a synthetic 229-frame board: base vs the whole patch 2,526 → 1,305 ms (M3 Pro, 2026-11-26); the cache alone, same viewer with and without it, 1,937 → 1,066 ms on All pages (−45%) and 230 → 137 ms on one page (−40%) (Apple M5, Chrome 158, 5 interleaved runs, p50; the engine without the cache is byte-identical to the base's; `docs/VIEWER.md` "Measurements — 2026-11-27"), so the compute AC (≥ 20%) is met on this fixture. Not measured: the plan-building share of the profile after the change (AC ≤ 2%), the native bench (item 1), `parity.mjs --gpu` and the ~7k-node test instance. Items 3 (prebuilt paint/paths) and 4 (build flags, allocator) open. Packaged 2026-11-17 from the Vello render speed survey (measured on the test instance, see Context).
**Depends on:** Task 30 (implemented). Uses Task 36's measurement script `web/tests/engine-load.mjs` for in-browser numbers (if Task 36 is not done, build the minimal worker measurement described in Task 36 item 1). No technical dependency on Task 34, but after Task 34 the server export (`Painter::Vello`) shares `paint-vello` and so gets the same benefit and its parity must be kept.
**Owned scope:** `crates/text/src/` (`shaping.rs`, `font.rs`, `measure.rs`, a new cache module) + `crates/text/tests/`; `crates/paint-vello/src/` (`color.rs`, `encode.rs`, `geometry.rs`, `stroke.rs`, `shadow.rs`, `text.rs`, `tests.rs`); `crates/engine-wasm/src/` only if the `Prepared` API has to change (keep `Prepared::new(&PaintScene)` if possible); optional phase: `scripts/build-engine-wasm.sh`, `Cargo.toml` (profile), `Dockerfile` (wasm build stage); the measurement section in `docs/VIEWER.md`. Do not edit `web/src`, server/storage, or the Canvas2D path/`renderer/`.
**Workspace:** new branch from `main` after Task 36. `crates/engine-wasm` currently has uncommitted changes from another session: do not commit on their behalf, do not stash/reset.

## Context

Read-only survey 2026-11-17, checked against the code. Measurement environment as in [Task 36](36-viewer-cold-load-quick-wins.md) (test instance on port 3900, project with 7,411 nodes / 190 frames / 42 fonts, headless Chromium WebGPU, 1440×900 DPR 2, M3 Pro). Worker CPU profile: Chromium trace `disabled-by-default-v8.cpu_profiler`, `.wasm` keeps the name section so Rust function names are visible (self time only, flat wasm stacks; tracing raises CPU by ~15%). Engine method cost: wrapping `Engine.prototype` through Playwright `route` on the JS glue.

- Loading All pages: `compute` 1,449 ms for 190 frames (p50 6 ms, max 62 ms). Worker profile over the whole load (self, 4,206 ms trace): **building HarfRust shape plans ≈ 460–480 ms** (`OtMapBuilder::compile` 206, `langsys` 74, `ShapePlan::from_layout` 42, `add_lookups` 30, `sift_down` 46, GSUB/GPOS lists ~60) ≈ 11% of the trace time, ~30% of compute; dlmalloc ≈ 580 ms; sha256 (soft) 121; `paint_vello::color::parse` 117; `serde_json::Value` 110 (belongs to Task 35); SipHash ~80.
- Cause of the plan cost: `crates/text/src/shaping.rs:86` calls `harfrust::shape(&scaled, &mut buffer, options)` with `ShapeOptions::new()` without `.plan(...)`, so HarfRust builds a new plan on every shape; `shaping.rs:46` also builds a font instance (`shaping_font_at_size` → `optical::at_size`) on every call. `measure.rs` shapes `""` (`:27`), every wrap candidate not yet in `widths` (`:39`) and then shapes each line again (`:69`). On the paint side, runs are already cached by (text, size, spacing) (`crates/paint-vello/src/resources.rs:74-91`), so the cost is mainly in compute (viewer worker, server native compute, `crates/render`). HarfRust 0.15 already has `ShapePlan`, `ShapePlanKey::matches` (compares script, direction, language, features, feature variations, lookup coords) and `ShapeOptions::plan`.
- Encode while panning All pages at Fit (22,137 nodes, 5,373 layers, 11,966 Greek lines): draw p50/p95 11.6/13.3 ms. Worker profile (self, 1,876 ms trace): vello strip/tile/sort ≈ 31%, `vello_gpu` scheduling ≈ 12%, dlmalloc ≈ 10%, **`color::parse` 6%**, **path building** (`BezPath::push`, `geometry::append`/`node_path`) ≈ 5%, guillotiere layer atlas allocation ≈ 4%. Half resolution (¼ the pixels) is still 14.0 ms vs 14.7 ms: the cost is per node/layer, not per pixel.
- `color.rs:11-27` `parse` allocates a `String` every time (`to_owned`/`collect`) and is called per node per draw through `solid` (fill, stroke `stroke.rs:77/130`, shadow `shadow.rs:27`, text `text.rs:18/75`, `encode.rs:270/280`). `geometry::node_path` rebuilds the path on every draw (`encode.rs:215/253/351`, `shadow.rs:91`). `Prepared` (`encode.rs:77-106`, built once per scene at `crates/engine-wasm/src/frame.rs:134`) only holds children/roots/extents.
- Engine wasm build: O3, thin LTO, CGU 16, no `wasm-opt` (`scripts/build-engine-wasm.sh:15-22`); `.wasm` is 6.05 MB raw / 1.27 MB brotli, name section 1.27 MB raw (−93 KB brotli if removed); the default wasm32 allocator is dlmalloc (≈14% self time when loading, ≈10% when drawing). `wasm-opt` is not yet on the work machine.

## Goal

Compute and encode do less repeated work with byte-identical results: compute on All pages down ~25–30% (~−400 ms), encode at Fit down ~10–20% (estimate), and the same benefit for server native compute/export.

## Work

1. **Measure before changing.** Native: a small bench (an `#[ignore]` test or `examples/`) measuring compute (text measurement) and `paint-vello` encode on a fixture with a lot of text/fonts/instances of ~7k nodes (generated synthetically, not using `~/.fagent`), reporting p50 over several runs. Browser: `engine-load.mjs` (total `compute` of All pages, wrapping `Engine.prototype`) and `test:engine-board` / pan Fit All pages (draw p50/p95). Record the baseline.
2. **Shape plan cache** (`crates/text`): keep the built plan per font (the same `Font`, shared between clones), look it up by `ShapePlanKey` (script, direction, language, features: with/without `liga`/`clig` disabled, feature variations, lookup coords of the instance) and `matches`; pass it through `ShapeOptions::plan`. Consider also caching the instance by size (`shaping_font_at_size`) if measurements show it matters. The cache is bounded (entries/bytes), safe for multiple threads on the server (multi-threaded native lane), and does not change `PartialEq`/identity of `Font`. If convenient, remove the repeated shaping in `measure.rs` (lines that already have a width in `widths` still need a run for `ink_bounds`: remove only when the result stays the same).
3. **Prebuilt paint/paths** (`crates/paint-vello`): `parse` does not allocate; `Prepared` holds parsed colors (before multiplying alpha) for fill/stroke/shadow/text and the scene-space path of each node (built once, possibly lazily), and the encoder reuses them instead of parsing/building on every draw. Record the extra memory of `Prepared` on All pages (bytes/scene). Keep the signatures of `encode_prepared` and `Prepared::new`; if they must change, update `crates/engine-wasm` minimally and record the reason.
4. **Optional phase — wasm build and allocator** (only after items 2–3 are done; each change has its own measurement, keep what helps): fat LTO + `codegen-units=1`; `wasm-opt -O3` (add binaryen to the build toolchain and the wasm stage in `Dockerfile`, pin the version; split the name section into a side file for profiling, not served to the viewer); a global allocator faster than dlmalloc for wasm32; `FxHash` for hot maps (internal keys, not accepted from users). Report `.wasm` size raw/brotli, build time, `compute` and draw before/after. Apply only to the engine wasm; leave `scene-wasm` unchanged unless measured separately.
5. Update `docs/VIEWER.md` (section "Vello engine": before/after table, notes on the plan cache and `Prepared`).

## Acceptance

### Acceptance Criteria

- Compute oracle: with the cache on versus off (or versus the base commit), measurement and normalized scene JSON are byte-identical on all existing tests of `crates/text/tests` (captured, regressions, variable instances, script runs, whitespace) and the ~7k node fixture; add a plan cache test that reuses the right plan and does not reuse the wrong one when script, features (spacing ≠ 0), instance/optical size or font changes.
- Paint oracle: `vello_cpu` images are byte-identical before/after on the `paint-vello` fixtures (`tests.rs`) and the 12-frame board; `spikes/vello-gpu/parity.mjs --gpu` results are unchanged; if Task 34 already has a `vello` adapter in `spikes/native-paint-parity`, the export PNGs are byte-identical before/after.
- Measurements (same machine, p50, baseline re-measured in item 1):
  - Total `compute` when loading All pages down at least 20% (target 25–30%); self time of building HarfRust plans in the load profile ≤ 2% (from ~11%).
  - Draw when panning Fit All pages p50 down at least 10%, or record the cause if not reached; `color::parse` is no longer among the hot functions (≤ 0.5% self time).
  - Native compute of the ~7k node fixture goes down, record the number; if Task 34 is done, record the 12-frame board export time before/after.
- Optional phase (if done): every build/allocator flag kept has a measured improvement; all engine tests pass with the new build.
- `cargo test -p fagent-text -p fagent-paint-vello -p fagent-scene-wasm -p fagent-engine-wasm`, `cargo test -p fagent-server -- --test-threads=2`, `npm --prefix web run test:engine` pass; `cargo fmt` and clippy clean for the modified crates.

### How to verify

```bash
cargo test -p fagent-text -p fagent-paint-vello -p fagent-scene-wasm -p fagent-engine-wasm
cargo test -p fagent-server -- --test-threads=2
```

```bash
npm run build:engine-wasm && npm --prefix web run test:engine
REPAINT_SERVER_BIN=target/release/fagent-server npm --prefix web run test:engine-board
node web/tests/engine-load.mjs --base-url http://127.0.0.1:3900 --project <id> --pages :all:,pg-m6 --runs 5
node spikes/vello-gpu/parity.mjs --gpu
```

The test instance on port 3900 (DB `fagent`) is read-only; do not print `~/Library/Application Support/FAgent-test/config`. Generated measurement output is ignored; commit only the conclusion table and the rerun commands.

### Output

Code + oracle tests; native bench; before/after table in `docs/VIEWER.md`; log in `docs/agents/LOG.md`.

## Constraints

Do not change compute results (measurement, wrap, scene), digests or pixels. Do not change `web/src`, server/storage, or the Canvas2D path. Do not connect to the DB of the other running instance or to ports 3847/3857/3858/3877; do not touch `~/.fagent` or the real runtime. Do not push.
