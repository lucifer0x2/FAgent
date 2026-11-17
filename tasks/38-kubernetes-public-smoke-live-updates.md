# Task 38 — Kubernetes public smoke: update for `liveUpdates` in `/api/v1/viewer`

**Priority (2026-11-17):** low, backlog. The user no longer uses Kubernetes (decision 2026-11-17); do this when returning to Kubernetes or when cleaning up deploy files. Not part of the order 34 → 36 → 37 → 31/35 → 33.
**Status:** Not assigned. Found 2026-11-17 while reviewing `56db62b..HEAD` (item #1).
**Depends on:** commit `402788e` (public viewer live updates off by default). Does not touch `web/`, `crates/` or the server.
**Owned scope:** `docker/kubernetes-public-smoke.mjs`, `docs/KUBERNETES-PUBLIC-VIEWER.md`, `docs/COMPOSE-POSTGRES.md`, `docs/NATIVE-RUNTIME.md`. Do not edit server code, manifests or `docs/VIEWER.md`/`docs/SERVER.md`.
**Workspace:** new branch from `main`; do not commit other people's changes for them.

## Context

`402788e` added `liveUpdates` to `GET /api/v1/viewer`: the body is now `{readOnly, liveUpdates}`. The public listener defaults to `liveUpdates=false`; enable it with `FAGENT_PUBLIC_LIVE_UPDATES=on` or `--public-viewer-live-updates on`. The contract is in `docs/SERVER.md`, section "Public live updates".

Two places still promise the old body:

- `docker/kubernetes-public-smoke.mjs:50`: `assert.deepEqual(await json("/api/v1/viewer"), { readOnly: true })` will **fail on the first run** because the body now has an extra `liveUpdates`.
- Docs: `docs/KUBERNETES-PUBLIC-VIEWER.md:65` (`# {"readOnly":true}`), `docs/COMPOSE-POSTGRES.md:83` (expects `{"readOnly":true}`), `docs/NATIVE-RUNTIME.md:265` (final check result `{"readOnly":true}`; this is the historical record of one verification run, so only add a note that since 2026-11-17 the body has `liveUpdates`; do not rewrite the measured result).

## Work

1. Smoke: replace the exact assert with `readOnly === true` and `liveUpdates === (env FAGENT_PUBLIC_LIVE_UPDATES on ? true : false)`; by default (env unset) expect `false`. If the smoke checks public SSE, expect 403 `PUBLIC_LIVE_DISABLED` when disabled.
2. Update the three docs: example body `{"readOnly":true,"liveUpdates":false}`, mention the flag that re-enables it and link to `docs/SERVER.md#public-live-updates`.
3. Scan `rg -n 'readOnly' docker docs` for any other place that promises the old body.

## AC

- The smoke no longer matches the exact `{ readOnly: true }`; it passes with the default configuration and with `liveUpdates` enabled.
- The three docs state the new body and the configuration flag; server behavior is unchanged.
- No file outside the owned scope is touched.

## How to verify

- `node --check docker/kubernetes-public-smoke.mjs`.
- Run the smoke against a local public listener (temporary data root and DB; do not use `~/.fagent`, do not connect to other services/DBs) twice: default and `FAGENT_PUBLIC_LIVE_UPDATES=on`. If there is no Kubernetes cluster, state clearly which part was not run for real.
- `rg -n '"readOnly":true\}' docs docker` returns no result that promises the old body (except the annotated historical record).

## Output

Diff of the three docs plus the smoke, and a short report listing the commands run and the part of Kubernetes that was not run for real.
