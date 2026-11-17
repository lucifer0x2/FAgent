# Task 35 — Engine: resolve per frame on each revision instead of resolving the whole snapshot

**Priority (2026-11-17):** do after [Task 34](34-vello-cpu-export-unify.md) (Vello export, removing Node/Chromium from the runtime), [Task 36](36-viewer-cold-load-quick-wins.md) (cold load) and [Task 37](37-text-shape-plan-paint-caches.md) (shape plan/paint caches), per the user's decision. Order: 34 → 36 → 37 → 31/35 → 33.
**Status:** Not assigned. Packaged 2026-11-17 from the render chunk/incremental survey (stage A).
**Depends on:** Task 30 (implemented). Independent of Task 31 (tiles); it makes Task 33 cheaper (after this task the cost of one edit is proportional only to the changed frame; Task 33 then cuts the remaining network/layout/tile work).
**Owned scope:** `crates/engine-wasm/src/model.rs` (+ `model/tests.rs`), `crates/scene-wasm/src/engine.rs`, `crates/scene-wasm/src/content_key.rs` (add a scoped variant, keep the old API), possibly a small API in `crates/scene-input/src/scoped/` and `crates/scene-input/src/scene_diagnostics/` (typed diagnostics), tests in `crates/*/tests/`, the measurement section in `docs/VIEWER.md`. Do not edit server/storage, `web/src/` (except renaming a called function if needed), `renderer/`, or the Canvas2D path.
**Workspace:** new branch from `main` (which includes Task 30 and commit `7307fa3`).

## Context

Checked against the code on 2026-11-17:

- Every engine revision creates a new `Accepted { resolved: OnceCell }` (`crates/engine-wasm/src/model.rs:56-74`); first use calls `ResolvedNodes::new(&snapshot, THEME)` (`model.rs:24-30`) = `resolved_expanded_nodes` (`crates/scene-input/src/validate/refs.rs:150-170`): validates the whole snapshot (`Candidate::validate`), expands every instance and resolves props of every node. Measured natively on a ~7.4k node project (7,411): ~380–410 ms; in the browser the first resolve is ~0.5 s (`docs/agents/LOG.md`, entry 2026-11-17). Commit `7307fa3` removed re-resolving on every frame, but every edit still pays this whole cost even though only one frame changes. The survey estimates this part at ~70% of the cost of one edit (estimate, not measured separately).
- For each frame computed, `frame_input` is built three times: `frame_keys` (`frame_content_basis`, `crates/scene-input/src/content_key.rs:16-37`) for every placed frame, then `required_fonts` and `compute` (`crates/scene-wasm/src/engine.rs:56-64`, `113-138`), each time as a `serde_json::Value` and then `from_value` into `LayoutInput`.
- Diagnostics go through JSON: `compute_with_diagnostics_resolved` serializes `scene.nodes` to a `Value` only so `scene_diagnostics::append` (`crates/scene-input/src/scene_diagnostics/overflow.rs:9`) can read it back; when there is a note it also deserializes `input["nodes"]` back to `Vec<Node>` for `overflow_ids`.
- A scoped path already exists on the server: `ScopedSnapshot::prepare_input_with_scale` (`crates/scene-input/src/scoped/input.rs:14-33`) expands + resolves only the closure of one frame and then calls the same `frame_input`; the test `crates/scene-input/tests/scoped_input.rs` already compares scoped == full byte for byte on a small fixture. Note: `ScopedSnapshot::validate` still runs `Candidate::validate` on the whole snapshot; the trusted path `ScopedSnapshot::from_validated` (used in `crates/storage/src/revision_indexes/shared.rs:55-90`) skips this step, and data errors inside the closure return a per-frame error, not a panic (`crates/storage/tests/stored_corruption.rs`).
- Render speed survey 2026-11-17 (measurement method in [Task 36](36-viewer-cold-load-quick-wins.md)): cold load pays this cost too — `frame_keys` 553 ms on `pg-m6` (64% of the worker's busy time when loading that page) and 664 ms on All pages. This task is therefore also the largest item for first load of a page; add the cold load numbers before/after to the measurement table.

## Goal

An edit that changes one frame on a ~7.4k node project: the engine returns the new frame in a few tens of ms instead of ~0.5 s, and the scene is byte-for-byte identical to the whole-snapshot resolve path.

## Work

1. Measure before changing: split the time into validate / expand / resolve / `frame_keys` / `required_fonts` / `compute` for one edit on a ~7.4k node fixture (native and in the worker) as a baseline.
2. Replace `Accepted::resolved` (whole snapshot) with per-frame input through `ScopedSnapshot::prepare_input_with_scale` (or an equivalent scoped API). Record the trust-boundary decision explicitly: the snapshot the engine receives from the server was already validated on write, so use `from_validated` + a `ScopeIndex` built once per revision; corrupt data inside a closure breaks exactly that frame (as in `stored_corruption`), no panic, no blank board. If whole-snapshot validation is kept, prove with measurements that the goal is still met.
3. `frame_keys` also moves to scoped (the digest definition does not change; keep `CONTENT_BASIS_VERSION` or bump it with a reason). For the All pages board (every frame of every page) measure the total key cost; if it is still ~whole-snapshot, find a way to avoid recomputing the digest for frames whose authored closure is unchanged, and record how it was chosen.
4. Build `frame_input` once per frame: the result is shared by the digest, the fonts to load and compute (kept in per-frame/revision staging), removing the redundant `Value` → `LayoutInput` round trips.
5. Remove the JSON round trip of diagnostics: `append`/`overflow_ids` take typed data (scene nodes, input nodes); the diagnostics result keeps the same order and content.
6. Keep the old `scene-wasm` API (Canvas2D path, server) unchanged in behavior; name the new variants clearly.

## Acceptance

### Acceptance Criteria

- Oracle: on the fixtures (the existing `scoped_input` fixture, a fixture with nested instances, tokens/themes, a note that deliberately causes overflow, images, fonts, and the ~7.4k node fixture) for every frame: the scene (normalized JSON) and diagnostics of the new path equal the whole-snapshot `ResolvedNodes::new` path byte for byte; the `frame_keys` digests are equal. Per-frame errors (missing frame, over 2048 nodes, viewport over budget) keep the same error codes as before.
- Cost of one edit (changing one text node in one frame) on a ~7.4k node project, native and in the worker (Chromium, `performance.mark` in the worker): a before/after table by stage from step 1; after: engine receives snapshot → changed frame committed in tens of ms (target ≤ 50 ms native, record the worker number), instead of ~0.5 s.
- Unchanged frames are still `reuse`d by digest (existing tests in `engine-frames.test.ts`, `model/tests.rs` pass).
- No behavior change in the Canvas2D path/old `scene-wasm` API; `cargo test -p fagent-scene-input -p fagent-scene-wasm -p fagent-engine-wasm`, `npm --prefix web test`, clippy/fmt clean.

### How to verify

```bash
cargo test -p fagent-scene-input -p fagent-scene-wasm -p fagent-engine-wasm
```

```bash
npm --prefix web test && npm --prefix web run test:engine
```

~7.4k node fixture: generate synthetically in a temporary data root (with instances/tokens/styles similar to a real project); additionally cross-checking against the approved test DB `fagent` is allowed (read-only). Do not use `~/.fagent`, do not write private data paths into documents. Generated measurement output is ignored; commit only the conclusion table and the rerun commands.

### Output

Code + oracle tests; before/after table in `docs/VIEWER.md` (Vello engine section); log in `docs/agents/LOG.md`.

## Constraints

Do not change scene semantics, digests or diagnostics as seen from outside. Do not touch the real runtime or `~/.fagent`. Do not push.
