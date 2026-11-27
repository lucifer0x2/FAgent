# Task 39 — Render a frame at a past revision

**Priority (2026-11-17):** backlog, not placed in the order 34 → 36 → 37 → 31/35 → 33.
**Status:** Not assigned. Recorded from agent-user feedback on known limitations.
**Expected owned scope:** `crates/storage` (commit/snapshot history), `crates/server/src/jobs/input.rs`, the `render_frame` schema, `docs/SERVER.md`. Do not touch the `web/` viewer.
**Workspace:** new branch from `main`.

## Context

`render_frame` accepts `revision`, but `jobs/input.rs::check_request` only accepts the snapshot's current revision; an older revision returns `REVISION_UNAVAILABLE`. Storage keeps only the current state (`nodes`, `tokens`, `styles`, `assets`, `fonts`) and a forward-only log `commits(ops_json, summary_json)`; there are no before-images or milestone snapshots. History can be truncated (reading missing history returns `RESET_REQUIRED`).

Consequence for workers: to get a "before" image to compare with "after", every frame that will be edited must be rendered **before** the first write. On a 20-frame redesign task, that takes the first ~8 minutes (20 images) and cannot be parallelized or deferred.

## Approach (choose when assigned)

1. **Milestone snapshots + forward replay:** store a compressed snapshot every N commits (or by time); rebuild revision R from the nearest snapshot ≤ R and apply `ops_json` up to R. Must handle deleted assets/fonts (keep bytes by `content_hash` until retention expires).
2. **Per-commit before-image:** store the old value of every key written, and rebuild backwards from the current state. Cheap when going back a few revisions, expensive when going far back.
3. Minimum: keep at least the last K revisions or T hours (configurable); outside that window still return `REVISION_UNAVAILABLE`, together with the oldest revision that can still be rebuilt.

## Expected AC

- `render_frame {revision: R}` with R inside the retention window gives exactly the same pixels as rendering when R was still current (compared with a cached job of the same identity).
- Outside the window: a clear error that names the oldest usable revision.
- Reads (`find_nodes`/`get_design_context` with `revision`) share the mechanism if it makes sense; state explicitly if deferred.
- Storage cost and rebuild time are measured on a large project (Task 24 benchmark) and recorded in the report.
