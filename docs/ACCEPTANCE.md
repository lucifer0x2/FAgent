# FAgent — acceptance map

Responsive on the Compose base, 2026-11-04: the `%`/breakpoint integration passed
70 Rust tests, production Clippy/fmt and 10 independent Chromium pipeline tests.
Server/layout-step images were verified on a PostgreSQL copy: Training,
Responsive Lab, the 721/720/390px thresholds, MCP/inspect/PNG and the desktop/mobile viewer.
The four snapshots and their 84 binary assets were left unchanged. No real rollout yet; the user
decided to install the real app themselves, and the agent does not deploy into the 3857/3858 services.
See the [integration and canary report](RESPONSIVE-COMPOSE-REPORT.md).

Task20, 2026-11-03: local Kubernetes on OrbStack built and deployed the real
PostgreSQL runtime, one app replica and a 2 GiB PVC. HTTP/MCP/PNG render, persistence
across app/DB restarts, apply keeping the Secret/PVC, SIGINT and DB-outage readiness were verified.
The coordinator independently verified Host/Origin, MCP/readback and ETag304. Loopback 3857 only;
no HA, migration, automatic backup or external cluster yet. See the [runbook and evidence](KUBERNETES.md).

Task20b adds a read-only Kubernetes listener on 3858 via an opt-in overlay and an origin in the
ConfigMap. Verified public reads/font/scene/PNG/SVG+PNG export, unchanged snapshots,
34 MCP/mutation cases blocked; the coordinator independently verified Host/Origin and local MCP on 3857.
Funnel/Serve not enabled; see the [public listener runbook](KUBERNETES-PUBLIC-VIEWER.md).

Task19 brought the product-domain PostgresBackend into StorageBackend/AppState/router.
The CLI defaults to SQLite. `DATABASE_PROVIDER=postgresql` requires `DATABASE_URL`. Task19's
in-process Moka snapshot cache was removed in Task 41 phase 4: latest-revision reads come from
node memory, which serves a resident copy only when PostgreSQL confirms namespace, canonical ID,
generation and revision. See [how to run](../README.md) and the
[snapshot caches](MOKA-SNAPSHOT-CACHE.md).
The Task15–17 milestones below record the historical scope of the spikes and the initial interface.

Task17, 2026-11-02: the PostgreSQL certified scalar delta commit-before-ACK spike
and Redis opt-in HOT reads passed core10, PGv133/v234, Node26 plus review/raw audit
and a real Redis lifecycle. The coordinator verified independently; code/binaries were frozen across
three writer1 stages and the HOT-read counterfactual. Cells/regressions and cache limits
were published.
The product runtime is still SQLite; no full PostgreSQL/MCP port or data migration yet.
Task18 Redis-primary/write-behind is only a research backlog.

Task16, 2026-11-02: runtime/jobs/export now use the shared StorageBackend/Storage/
ProjectHandle and the SQLite adapter; an independent RAM backend was injected over HTTP/MCP.
The coordinator verified storage/cache8, server15, lint/fmt and E2E16 groups + live workspace.
No production PostgreSQL/real-data migration yet. See [`STORAGE-BACKEND.md`](STORAGE-BACKEND.md).

Task15, 2026-11-02: the user chose PostgreSQL; the independent spike was verified by the coordinator
across the full matrix of 100/5000 nodes×1/4/8 writers, history1000, recovery and migration/backup
fixtures. The fast path/retry optimization and two indexes were verified further with the full 9 phases, real
EXPLAIN and the matrix on a fresh DB.
The next bulk SQL optimization passed the coordinator's full 9 phases/19 contract/18 Node and a wire probe:
batch1/3/500 changedIDs use6 fixed statements; matrix latency results are mixed,
and fewer queries are not treated as an SLA.
The product runtime is still SQLite; full PostgreSQL storage/MCP, real-data
migration, Kubernetes rollout and HA are not accepted.

Task04 was reviewed and independently accepted by the coordinator: see
`docs/TASK04-REPORT.md` and `artifacts/e2e/wrap.json`. Opt-in row wrap passed
pipeline16 tests (keeping the previous11), storage41 tests/14 suites, server19 tests (keeping the previous18),
production E2E16 groups (keeping the previous15), strict library Clippy for layout/storage/server.
The coordinator ran layout7/7, pipeline16/16, storage41/41, server19/19 and Node E2E16
groups on the final builds; E2E completed 2026-10-28T09:11:22.774Z, cleanup=true.
Fmt, JS format/syntax, diff and size checks pass. The older results below are history.

New fixture min280/gap16/padding24: 1040→3 cards/1 row, 640→2+1, 390→1+1+1;
row hug geometry 80/156/212, real text 128/224/320. Bounds of all descendants
stay inside the frame/card, line widths fit, no overlap; no node is exempted because of clipping.
Restart keeps authored wrap/revision/bounds and PNG 0 RGBA differences at all 3 viewports;
viewer GET-only 0 browser errors, scene/PNG 0 differences at 1040×844 and 390×844.
Images at `artifacts/e2e/wrap-{1040,640,390}.png` and `wrap-viewer-{1040,390}.png`.
The coordinator and worker viewed the real desktop/mobile PNGs. The old limits of the welcome dashboards still hold:
sidebar/body minima, fixed chart/progress or fixed text heights are not re-authored.
Task04 acceptance scope is row wrap (align/justify were added later in P08a). The real 3847
was not touched; temp processes/data were cleaned up. The detailed report has commands and lint caveats.

Requirements source: `PLAN.md`. This document is managed by the coordinator session. Accepted
2026-10-28: **MVP M0–M2 passes on the recorded local runtime**; M3 and M4 pass
the parts within the published scope below. Do not generalize fixtures into guarantees for every
browser/OS, hardware power loss or features outside scope.

| Milestone | Required evidence | Status |
| --- | --- | --- |
| M0 pipeline | Taffy + bounded batch measurement/cache; two widths, accented-text wrap/hug; shared scene/renderer/font manifests; missing font/delayed image strict readiness | Passes fixture scope: coordinator's last run 11/11 Chromium pass, real font hash matches; see spikes/pipeline/RESULTS.md and docs/RESOLUTION.md |
| M0 storage | Patched SQLite runtime, WAL/FULL/FK; budget before benchmark; small/large create/update/reparent; 1/4/8 clients + reader; queue/conflict/WAL/checkpoint; correctness/restart | Passes the published workload; patched SQLite 3.54.1, latency/queue under budget; successful-batch measurements are not treated as end-to-end retry latency or physical writes; see spikes/storage/README.md |
| M1 domain/storage | Create/open project guards; typed atomic ops, revision/idempotency; resource refs; durable assets; tokens/styles resolver | Coordinator ran full storage after M3/M4: 38 tests / 13 suites pass, 12.66s, with fault-injection; MCP/browser see the E2E row |
| M1 API/MCP | Streamable HTTP MCP schemas; list/create, outline/subtree/context/resources, apply/import/render/inspect; HTTP versioned snapshot/SSE/assets/jobs; caps and client-safe errors | Passes: coordinator's server suite 9/9 pass; actual official rmcp transport + Axum + Chromium worker. See docs/SERVER.md |
| M1 viewer | Light editor shell, collapsible layers/inspector, project/page/frame/viewport, read-only selection/highlight/pan/zoom/fit; explicit scene revision | Passes: frontend4 state-test fixture/9 earlier smoke groups and real production E2E in the last run; the coordinator viewed desktop/mobile images; see docs/VIEWER.md |
| M1 E2E | MCP → DB → viewer → PNG → restart/reload keeps IDs/refs/revision; invalid batch does not commit | Passes: Task05 extended to 15 groups, coordinator's independent check pass. RGBA 0 differences prove the renderer is deterministic; fit/reflow capability is reported separately below. Snapshot/assets unchanged; see artifacts/e2e/evidence.json |
| M2 transaction/recovery | Concurrent base revision; lost-response retry/key reuse; crash before/after commit; asset publish/commit durable/orphan | Passes: coordinator's full storage including abrupt-exit/asset recovery; server concurrent conflict + valid changed-key reuse; E2E discarded-response replay and invalid batch rollback |
| M2 events/jobs | Snapshot/replay drops no commit; cursor reset; reconnect; old job does not replace a new scene; worker failure/missing resource statuses | Passes the published seam: coordinator's server 9/9, pipeline11/11, live SSE/restart E2E; earlier frontend stale/reconnect fixture. Crash/timeout orchestration uses a protocol fixture; actual Chromium verifies render/resource. Retention is lifetime, no pruning |
| M3 context/resolution | Cursor pagination, consistent revision, typed alias/theme cycle checks/invalidation; layout fixtures and diagnostics | Passes v1 scope: resolver9/9, pipeline11/11, server cursor/theme suite and live token invalidation; authored/resolved clearly separated. No deep rule explanation/diff or style aliases yet |
| M4 hardening | Versioned migration keeps the DB on error; consistent backup DB + resource manifest; round-trip new project/revision reset; hash/archive/symlink guards | Passes the storage API within the published scope: 12 M4 tests in the independent full storage run, including writer/export/owned snapshots; no GC or backup UI/CLI/MCP. See docs/BACKUP.md |

## Initial checks of the resumed session — 2026-10-28

- Read the working tree/checkpoint, kept existing changes. Three new workers continued resolver/layout fixtures, hardening and server; the coordinator only coordinated/verified.
- `resume_resolution` ran `cargo test -p fagent-storage --offline --test resolution -- --test-threads=2`: **5/5 pass**. This is a worker result, not a substitute for the independent check or for all of M3.
- `resume_server` initially found a partially moved `ReadRequest.project_id` error; fixed, and the final check below replaces this initial state.

## Independent checks by the coordinator in the resumed session

| Command | Result |
| --- | --- |
| `npm run test:pipeline` | 11 passed, 0 failed/skipped/cancelled; 2.37s; loopback escalation approved |
| `cargo test -p fagent-storage --offline --features fault-injection -- --test-threads=2` | 38 passed / 13 suites, exit 0, 12.66s |
| `rtk bypass cargo test -p fagent-server --offline -- --test-threads=1` | 9 integration tests passed in one run; context1 + failures3 + render2 + transport3; exit0; actual Chromium and loopback escalation approved |
| `rtk bypass cargo clippy --offline -p fagent-storage -p fagent-server --lib --bin fagent-server -- -D warnings` | Pass, exit0; both production libraries and the server binary |
| `cargo fmt --all --check` | Pass, exit0 |
| `cargo build --offline -p fagent-server && node spikes/e2e/run.mjs` | Pass, exit0 on the final binary; 12 check groups, completed 2026-10-28 05:16:19 UTC; temp project/process cleanup succeeded |

The E2E worker also ran `npm run test:e2e`: build renderer/layout/server and the production
web (with TypeScript check), then E2E pass. The coordinator used these builds and rebuilt the
server last for the independent run. No demo data is written to `~/.fagent`.

RGBA: 640×480 revisions 2/3 and after restart compared 1,228,800 channels per run, 390×844 compared
1,316,640 channels, all 0 differences. Scene/line list/provenance were also compared intact.
Real images viewed: `artifacts/e2e/desktop.png`, `mobile.png`; backend PNG at
`frame.png`. Script and limits at `spikes/e2e/README.md`.

**Limit of the pixel measure:** 0 RGBA differences at a narrow viewport only prove that the
renderer gives the same result on the recorded runtime. It does not prove all content
fits in the frame, that the design reflows, or that it is usable on mobile. `clip:true` can
hide all overflow in both images while the pixels stay equal. The viewer's scrollWidth check
only concerns the viewer shell and does not replace the design bounds check.

Size review of all hand-written Rust/TS/TSX/JS/CSS in crates/renderer/worker/
web/spikes: no file exceeds 250 lines; the largest source/test is 242 lines.
Generated lockfiles, `web/dist` and the renderer bundle are not subject to the hand-written limit.
`git diff --check` passes. The shared server test helper has a dead_code warning in
test binaries that use only part of the helper; production strict lint has no warnings.
The web build still has a chunk warning of ~574.59kB, which does not block the build.

Published limits: one Chromium fixture/runtime; arbitrary authored history, states
beyond default/hover/pressed/focus/disabled, hot runtime replacement, log expiration/GC and backup UI/CLI/MCP
are not supported. Backup/restore is a Rust storage API. 4096 tokens were checked by validator/
readback, with no load test of 4096-token mutation. The benchmark does not measure physical writes
or end-to-end retry latency. No commit/push; the working tree keeps old/new changes.

Review fixed and added regressions: a failed render cache allows retry; a frame root
does not carry its parent page into the layout input; the bundle keeps valid sort-key gaps after
delete/reparent and accepts a bundled-font copy as a referenced resource.
Retention metadata is now read in the same snapshot transaction.

## Reporting rules

P27 adds flat visual states and whole-frame render per state, keeping the default layout.
The contract is at [Interaction states](INTERACTION-STATES.md); AC evidence,
pixel diff, cache and limits are in the [P27 report](P27-REPORT.md).

### Addendum Task 01–02 — 2026-10-28

- Task01: coordinator's server12/12 integration tests pass, including MCP resource list/read,
  capability, URI errors, tool compatibility and whole-frame diagnostics before
  filter/pagination. Strict production Clippy, fmt and diff-check pass.
  See `docs/SERVER.md`.
- Task02: the coordinator ran `node samples/welcome-stacks/verify.mjs` pass on a temp data root/random
  port, cleanup=true. The coordinator viewed PNG1040/800/390. Desktop differs by
  **0 pixels**, MAE0; body shrinks864→624→480 and cards248→168→120, sidebar stays176.
  There are 2 free overlays (logo/chart plot), the rest use stacks. Viewport390
  is clipped because the min total is656; the chart is not rescaled, text may overlap at min.
  The full report is at `samples/welcome-stacks/README.md`, evidence at
  `artifacts/sample-stacks/verification.json`. Do not treat this as wrap/breakpoints
  or a complete mobile layout. After QA and the user's agreement, the real project
  `fagent-welcome-stacks` was created at revision1. The live desktop PNG matches QA with 0 pixels;
  the viewer compared PNG 0/2995200 channels different, the API only GET, no page error.
  The coordinator read it back through the connected MCP and viewed the real screenshot; the old sample stays at revision1.
  Live evidence is stored separately at `artifacts/sample-stacks/live/`.

### Addendum Task05 — multi-viewport acceptance, 2026-10-28

The worker ran `npm run test:e2e` for real, successfully (15 check groups; all12 old groups kept),
Chromium156.0.8078.4 / Node24.21.0 / MCP2025-11-25. The first sandbox run built
successfully but hit EPERM binding loopback; the same command was rerun through an
approval and exited0. The coordinator independently verified bounds tests3/3 and
`node spikes/e2e/run.mjs` 15 groups all pass; the coordinator's run completed
2026-10-28T08:33:57.842Z, temporaryDataRemoved=true.
Machine results at `artifacts/e2e/responsive.json`; report at
`docs/TASK05-REPORT.md`.

Each sample was created through MCP in a temp data root/random port. Render and
`inspect_layout` verify all 70 free descendants /180 stacks descendants, not counting the frame
root; comparing x/y/width/height, epsilon 1e-6 logical px. Bounds are not cut by clip.
Re-rendering after restart creates a new job, keeps the scene key/bounds, and compares decoded RGBA with the
PNG before restart at all three viewports, tolerance 0.

| Viewport | Free: pixel / nodes overflowing frame / reflow from 1040 | Stacks: pixel / nodes overflowing frame / reflow from 1040 |
| --- | --- | --- |
| 1040×720 | Pass 0 / 0 / baseline | Pass 0 / 0 / baseline |
| 800×720 | Pass 0 / 21 / no (0/70 nodes changed) | Pass 0 / 0 / yes (106/180 nodes changed) |
| 390×844 | Pass 0 / 52 / no (0/70 nodes changed) | Pass 0 / 104 / yes (116/180 nodes changed) |

Free keeps the sidebar `{x:0,y:0,width:176,height:720}` and the focus card
`{x:744,y:148,width:248,height:112}` at every viewport. At 390, the date overflows right by 238px,
greeting 418px, new-task-button 602px and focus 602px; clip only hides them from the PNG.
This is a fixed design, not a renderer bug.

Stacks body shrinks 864→624→480px, stat card 248→168→120px. At 800 no node exceeds the
**frame**, but 10 nodes exceed the **weekly-chart card** (chart-grid-0 exceeds by 212px)
because the plot keeps hard coordinates; this must not be called frame overflow. At 390, total
sidebar176 + body min480 =656px makes the body overflow right by 266px. 104 nodes overflow the frame;
wrapped text may also overlap vertically, outside this horizontal overflow check.
Both samples **do not yet meet mobile design**; having reflow does not by itself prove fit.

The report stores every overflowing node with `nodeId/leftPx/rightPx/totalPx`, all bounds,
`changedNodeIds` and the changed dimensions, scene/PNG provenance, dimensions,
count and a separate pixelPass. `spikes/e2e/known-overflow.json` locks exact IDs and
px counts from the reviewed Task02 evidence; E2E does not regenerate the allowlist. Any new
overflowing node, lost node or different px fails the check so the design change gets reviewed.
Overflow is not ignored because of parent clip or node type.

`node --test --test-concurrency=1 spikes/e2e/bounds.test.mjs`: 3/3 pass, checking
left/right overflow, root-only change without reflow, and scope must not be empty/missing.
Temp browser/MCP/backend/worker were closed, the temp data root deleted; the user's server
3847/PID12919 was left untouched. No engine, sample design or Task03 changes.

### Addendum Task03 — multi-viewport context, 2026-10-28

The coordinator independently ran the full server: **18/18 integration tests pass**, including 6 new tests
with actual MCP/temp roots for the two samples, full-scope bounds/pagination/cursor/compat,
cold text worker, timeout, queue and resource recovery. Strict production Clippy,
fmt and diff-check pass; the largest server code file is 233 lines. API `viewports` 1–4,
total 16Mpx, a cold read returns pending/reflow:null; reading again when ready returns bounds
per viewport and reflow, not counting the frame root in the comparison. Legacy requests stay
compatible. Cost and contract at `docs/TASK03-REPORT.md`, `docs/SERVER.md`,
`docs/CONTRACT.md`. The real server 3847 was not restarted so it has not loaded delta03.

- Tests must go through the real seam; a mock does not replace evidence from the browser worker or durable SQLite.
- Do not use a pretty image as evidence of persistence/events. Scene line lists and provenance must be checked separately.
- The coordinator's data tests are temporary; do not use real projects in `~/.fagent`.
- Distinguish not implemented, not verified, failed and passed. Do not record a whole milestone as passed just because one happy path is green.
- Record command, runtime, fixture, result and limits in the task report; keep a summary log in `docs/agents/LOG.md`.

### Addendum P08a — stack align/justify, per-edge padding (night batch 2026-10-28)

Checks: `cargo test -p fagent-layout`
(`stack_tests`, `stretch_tests`), `-p fagent-storage --test stack_layout`, `-p fagent-server
--test stack_context`, `npm run test:pipeline` (`stack-align.test.mjs`). No `alignSelf`,
`space-around/evenly`, baseline, margin or column wrap. Percentage sizes
and breakpoints were added separately per the [responsive contract](CONTRACT.md#responsive-layout).

### Task14 — instance schemes and Structured Operations, 2026-11-02

P1–P5 implemented and independently verified: scoped frame/instance themes, merged paint
overrides/root paint, MCP pure JSON inbox batches and dry run, an MCP-first authoring
guide with schema-correct fewshots. The coordinator verified storage48/layout29/server25/web26,
real Chromium preview smoke, typecheck/fmt/diff and a synchronized debug build, all passing.

U07 golden/payload AC not met: the design worker rebuilds on the new API, then compares images at
fuzz≤3% and re-measures; ~4KB/scheme is still a prediction. The P6 contrast check is not implemented.
No release/restart of the real service; SVG keeps stroke/shadow omission diagnostics.
