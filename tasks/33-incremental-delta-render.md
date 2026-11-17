# Task 33 — P4 Incremental: delta from the server → dirty layout → dirty tiles

**Priority (2026-11-17):** do after [Task 34](34-vello-cpu-export-unify.md) (Vello export, removing Node/Chromium from the runtime), [Task 36](36-viewer-cold-load-quick-wins.md) (first load) and [Task 37](37-text-shape-plan-paint-caches.md) (shape plan/paint cache), per the user's decision. Order: 34 → 36 → 37 → 31/35 → 33.
**Status:** NOT ASSIGNED. Packaged 2026-11-16. Phase P4 of
[VELLO-GPU-RENDERER-RESEARCH.md](../docs/VELLO-GPU-RENDERER-RESEARCH.md).
**Depends on:** Task 31 (tiles), Task 28 node-memory (`Delta`, instance/master index).
**Owned scope:** server: a new delta route/SSE payload in `crates/server/src/` (events, reads), `docs/CONTRACT.md`,
`docs/SERVER.md`; engine: `crates/engine-wasm/` (ProjectStore, layout cache), possibly adding an incremental API to
`crates/layout`/`crates/scene-input` (keeping the full result unchanged); web: `web/src/hooks/useProjectLive.ts`,
`useSnapshot.ts`, `web/src/engine/`; matching tests.
**Workspace:** new branch from `feat/vello-renderer` after Task 31.

## Context

The SSE `commit` event already carries `changedIds` (`crates/server/src/events.rs:78`) but the client re-GETs the whole snapshot
(`web/src/hooks/useSnapshot.ts:29-37`); a frame whose digest changed is fully recomputed (a fresh Taffy,
`crates/layout/src/engine.rs:21-120`) and the whole frame is redrawn. `node-memory` returns a `Delta` (node/note upsert-delete,
token, style) per batch (`crates/node-memory/src/ops/delta.rs`).

**Addendum 2026-11-17 (incremental render survey):**

- Do [Task 35](35-engine-frame-scoped-resolve.md) first: currently every engine revision resolves the whole snapshot
  (~0.4–0.5 s on ~7.4k nodes) even when only one frame changed; after Task 35 the rest of an edit is network + frame layout + draw,
  which is exactly the scope of this task.
- The web ignores the SSE `changedIds` (`commitAction` in `web/src/state.ts` only looks at the revision), re-GETs the whole snapshot, then
  sends the whole snapshot to the worker (`web/src/engine/client.ts` `sync`).
- `get_changes` already exists but **only over MCP** (`crates/server/src/reads/changes.rs`, `mcp.rs:92`), with no HTTP route;
  it returns IDs grouped by kind (+ sanitized `operations` when `includeOperations`), not post-commit rows; it has
  budget/limit and `RESET_REQUIRED` when history is missing. Item 1 below should reuse this logic (history, pagination, gap)
  for the HTTP route, adding rows, rather than writing new logic.
- Item 4 (dirty tile) relies on per-frame tile invalidation that the first slice of Task 31 delivers; this task narrows it down to per node.

## Goal

Edit one node: only the changed rows cross the network, only the affected subtree recomputes layout, only the tiles intersecting the changed area re-raster.

## Work to do

1. Server: `GET /api/v1/projects/{id}/changes?since=R` (or an SSE payload carrying rows when small) returns the changed node/note/token/
   style/resource rows from R to the current revision, size-limited; over the limit / missing history → the client
   falls back to GET snapshot. Same permission/privacy checks as the snapshot; the public viewer only for public projects.
2. Engine ProjectStore: apply rows in revision order, check continuity; determine the affected set (token/style → nodes using it;
   master → instances; reparent → both parent trees).
3. Incremental layout: keep the Taffy tree + text measurement results per frame; `mark_dirty` the changed node and its ancestors; re-measure only changed
   text. Oracle: the incremental result equals a full recompute (scene compared byte for byte) over random edit sequences.
4. Dirty rect: old bounds ∪ new bounds (including shadow/stroke/overflow/ink) → intersecting tiles are marked; only those tiles are re-rastered.
5. Measure edit → pixel on the 12-frame and 10k-node fixtures.

## Acceptance

### Acceptance Criteria

- Editing 1 text node (12-frame fixture): edit response → pixel < 50 ms (currently 148 ms), the number of re-rastered tiles ≤ the number of tiles intersecting the
  changed bounds; no GET snapshot.
- Incremental oracle = full: ≥ 1,000 random edits (props, text, reparent, token, master) with no mismatch.
- Revision gap/reset/history too long → snapshot fallback, displayed correctly.
- Server SQLite + client tests pass; PG runs when a one-off temporary DB is available (do not use PG 2703).

### How to verify

```bash
cargo test -p fagent-server -p fagent-storage -- --test-threads=2
```

```bash
npm --prefix web test
```

### Output

Code + tests; the new contract in `docs/CONTRACT.md`; measurements in `docs/VIEWER.md`; log in `docs/agents/LOG.md`.

## Constraints

Do not change the commit/LWW semantics of Task 28. Do not touch the real runtime or `~/.fagent`. Do not push.
