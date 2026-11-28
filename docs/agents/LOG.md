# FAgent coordination log

Append-only. Rules for entries:

- One dated heading per task: `### <task> — YYYY-MM-DD`; each entry is a few lines.
- Record only decisions, final results, blockers and links to reports. Do not copy content that already lives in `docs/TASK*-REPORT.md`, `tasks/`, test output or git history; no intermediate numbers.
- Log at every assignment, result received, blocker found and before stopping. Never write secrets or the user's private data paths.
- When this file exceeds ~300 lines, delete closed entries; record any technology or approach that was dropped in `docs/REJECTED-APPROACHES.md` first.

### Task 30 follow-ups (Vello engine worker) — 2026-11-17

- Task 30 accepted: Worker engine; Vello is the viewer default since 2026-11-17 and `?renderer=canvas2d`/`?scene=server` keep Canvas2D. Details and measurements: `docs/VIEWER.md`, section "Vello engine".
- Still open: the `npm run test:e2e` harness is broken (`compareViewer`; already failing before Task 30); the Layers panel is virtualized but the ~90 ms long task on large boards was not re-measured; Safari 26 has not been measured.
- Safari WebGPU renders blank while WebGL2 works. Cause unconfirmed (hypothesis: `transferToImageBitmap` of a WebGPU OffscreenCanvas in a Worker returns an empty image). Mitigation: a 16 px present probe at attach, uncaught GPU errors caught, and automatic fallback to WebGL2 with a note; the `gpuFault=blank|throw|error` hook simulates it on Chromium. Not tried on real Safari/iOS.

### Task 34 — Vello CPU export — 2026-11-17

- Decision: export (`render_frame`, `export_nodes`) moves to `vello_cpu` plus native compute; the build-time env `FAGENT_RENDER_BACKEND=chromium|vello` (default `chromium`) picks the backend, and the native build uses `vello`. The `rust`/tiny-skia backend is removed (`paint-core` stays). Final goal: no Node/Chromium in the runtime. Native `vello-gpu` (phase 5) is skipped because raster is under 40% of job time.
- Phases 1–3 done and phase 4 partly done; merged into `main`. Parity with Chrome is not exact (text up to ~6.6%); the user accepted it. Report: [PURE-RUST-EXPORT.md](../PURE-RUST-EXPORT.md).
- Not run yet: `docker build`, a real `build-release.sh`/`deploy.sh`; the Kubernetes probe still uses `node`. `cargo clippy --all-targets` on the server fails in files this work does not touch; `native_layout_parity` fails independently of it.
- Waiting on the user: whether to drop the runtime `RENDER_BACKEND` override.

### Render speed — Tasks 35, 36, 37 — 2026-11-17

- Priority order set by the user: 34 → 36 → 37 → 31/35 → 33.
- Task 36 (web: bounded progressive drawing, removal of the ~300 ms Suspense delay, parallel font/image loading, `MessageChannel` yield) targets All pages cold load ~4.0–4.8 s → ~2.5 s. Constraint: do not preload App or fetch project data before `ViewerGate` opens (`app-shell-smoke.mjs` enforces it).
- Task 37 (Rust: HarfRust shape plan cache, paint/path prebuilt in `Prepared`; wasm build flags and allocator are an optional phase). Task 35 (frame-scoped resolve per revision, byte-exact oracle). Small-node LOD at low zoom goes to Task 31; compute pool, scene cache in IndexedDB and `/snapshot`/JS compression go to Task 32.
- Open: Tasks 31, 32, 33, 35, 36, 37 not assigned yet.

### Task 38 — Kubernetes public smoke — 2026-11-17

- Code review of `56db62b..HEAD` found 8 issues; 7 are fixed with tests. The eighth is shelved as [Task 38](../../tasks/38-kubernetes-public-smoke-live-updates.md): the Kubernetes smoke and three docs still assert the old `/api/v1/viewer` body, which gained `liveUpdates`. Low priority because the user no longer uses Kubernetes.
- Public viewer live updates are off by default (`FAGENT_PUBLIC_LIVE_UPDATES=on` enables them); contract in `docs/SERVER.md`, section "Public live updates". Viewer overscan is the env `FAGENT_VIEWER_OVERSCAN`, default 0 (off), served through `/api/v1/viewer`.
- Not verified: Mac Mouse Fix and real touch screens for the gesture fixes.

### Agent feedback: known limitations — 2026-11-17

- Already on `main`: `import_asset` by inbox `path`, root frame `height:"hug"`, `apply_batch_file` (now also accepts `.ocd`), `import_assets` (1–50 items in one revision), `find_nodes` with `instances: reference|expand` (default reference; skips virtual instance children, reports `skippedInstanceMatches`).
- Rendering a frame at a past revision is not built: [Task 39](../../tasks/39-render-past-revision.md), backlog.
- Layout and instance limitations triage: one real bug (`crates/layout/src/tree.rs` treats a free container as an anchor only for percentage-sized children, ignoring `fill`); two cases mostly by design with gaps (stretch inside a hug parent, an unset axis treated as hug once stretched); `clip` missing on instances, by design; slots and breakpoints that resize children are missing features. Proposed order: A free-container anchor bug, B stretch/hug gaps, C `clip` on instances, D breakpoint resizing of children, E slots; waiting for the user to choose.
- PostgreSQL `apply_batch_file` returned `STAGING_UNSUPPORTED`; fixed by "read batch files from the inbox root on every engine", merged into `main` on 2026-11-17. Native deploy waits for the user.

### Repository restart — 2026-11-17

- At the user's request the repository restarts with fresh git history, English only. Removed: closed tasks 01–19b, their reports, research for dropped directions, `docs/recommend-features/`, `docs/agents/history/`, `docs/checkpoints/`, old spikes (`rust-export`, `responsive-proposal`). Dropped technologies are summarized in `docs/REJECTED-APPROACHES.md`.
- Non-English test strings and fixtures were replaced with English (accented Latin where a test needs diacritics); non-English Chromium-measured corpus entries were removed rather than translated.
- New rules in `AGENTS.md`: English-only content and a blocking "Public repository" rule. `scripts/check-public-content.sh` (run by `.githooks/pre-commit`) blocks local absolute paths, real tailnet hosts, secrets, non-example emails and the names in the untracked `.private-patterns`.
- Open-source scrub: client project names, IDs, slugs and snapshot hashes, real backup names, a real tailnet host and the migration scripts for the previous product were removed; machine notes moved to the untracked `CLAUDE.local.md`. Limitation IDs that pointed to the previous product's feedback list were replaced with plain descriptions.
- The README screenshots and motion GIF were removed because the demo frame's footer showed the previous product name. Open: the user will add new screenshots to `README.md`.

### Task 40 — Liquid glass shader — 2026-11-18

- Packaged [Task 40](../../tasks/40-liquid-glass-shader.md) (backlog) at the user's request: backdrop blur and a glass material (refraction, chroma, specular) through a custom WGSL pass plus a `vello_cpu` reference, because Vello 0.4 only executes Flood, GaussianBlur, Offset and DropShadow filters.
- Open: not assigned; phase 0 spike decides segmented rendering vs prefix re-encode and the WebGL2 path.

### README redesign and screenshots — 2026-11-18

- README restructured after well-known open-source READMEs: hero GIF, tagline, quick links, gallery, one-line highlights, short quick start, agent connection, performance summary. Reference sections moved verbatim to `docs/GETTING-STARTED.md`, `docs/ARCHITECTURE.md` and `docs/PERFORMANCE.md` with relative links fixed.
- Closes the open screenshot item above: `docs/assets/readme/` holds `viewer-frame.png`, `all-pages-board.png`, `viewer-inspector.png`, `motion-preview.gif` and `motion-orbit.gif`, captured from a demo project authored through MCP.

### Task 41 — Node-memory engine — 2026-11-18

- Packaged [Task 41](../../tasks/41-node-memory-engine.md) at the user's request after the commit-path review: one engine struct in `crates/node-memory` with no database knowledge, always on, full validation once at load, diff-only validation per commit with per-op checks kept, error order among end-of-batch errors allowed to change, no pre-expanded instances.
- Decisions: struct name `NodeMemory`; invalid stored data is admitted, served, counted and logged with ids/codes only; spike 1b result (c) adopted, one persistent `ProjectState` over a hand-written radix trie (`imbl` allowed for the spike only); pure diff-only validation, a stored defect rejects a batch only when the batch's closure reaches it; the expanded-node limit follows `Limits.max_nodes`; phase 4 runs on `feat/node-memory-optimization`, which carries the whole task; `memory_snapshot`/`evict_memory` removed (2026-11-19).
- Phases 1–4 done 2026-11-18/19 (boundary, validator trait and admission, delta rules, cleanup of Moka, test-only oracles and shadow code). Results, measurements and behavior changes: [TASK41-REPORT.md](../TASK41-REPORT.md).
- Review fixes 2026-11-19: links into a frame whose recreated parent changed kind, the note limit on stored excess, flow names through a `flow:` index, per-master expanded-size counters, shared component-graph walk and commit staging; see the report's "Review fixes".
- CodeRabbit review of PR #5, 2026-11-19/20: 15 code findings fixed (`2a1e2f8`, `ba67292`, `dc76d06`); commit validation now expands one affected instance at a time; tests and review-only items deferred (local notes). See the report's "CodeRabbit review fixes".
- Merged to `main` in PR #5 (`74f3a87`, 2026-11-20); docs, report and spike 1b bench examples follow on `docs/task-41-docs-bench`.
- Open: phase 5 (lazy expansion cache, optional); ~900-instance edits validate in 15–22 ms at 200k; admission ~0.8–1.0 s per 200k load; removing write-path expansion entirely (rule 6 vs rule 7).

### CodeRabbit pull request review — 2026-11-18

- Added `.coderabbit.yaml` (schema v2, checked offline against `schema.v2.json`): English-language reviews, AGENTS.md as a code guideline, Betterleaks/TruffleHog/Presidio scanners on, and three `error` pre-merge checks (English only, no secrets or passwords, no local absolute paths). Feature review: per-area path instructions (storage, node-memory, scene-input, server, layout/text/paint, viewer) and four `warning` checks (task acceptance criteria from `tasks/`, tests for behavior changes, contract and docs in sync, no rejected approach). `auto_approvals` requests changes while a blocking check is open; authors cannot self-approve through `@coderabbitai approve`.
- CodeRabbit is advisory review on top of the deterministic gate; `public-content.yml` (check script plus gitleaks history scan) stays the blocking check.
- Open: the maintainer installs the CodeRabbit GitHub App on the repository and, if wanted, marks `public-content` and `CodeRabbit` as required checks on `main`. Public repositories under 10 stars get reviews only on `@coderabbitai review`.

### Schema rejections name the failing field — 2026-11-18

- Reported: `apply_batch_file` returned the generic schema `INVALID_REQUEST` for every call; the three required arguments were valid, but a bad operation in the JSON file (re-validated as `apply_batch`) or an `idempotencyKey` outside `[A-Za-z0-9_-]` produced the same message with no field.
- Fixed on `fix/schema-error-fields`: rejections carry the field path in the message and `error.field`, `opIndex` for operations; `oneOf` blames the variant accepting the most sent fields. Accepted inputs unchanged. Details in `docs/SERVER.md` (MCP section).
- Review follow-up: quoted key paths escape C1 controls (U+0085) too; on an unknown-field tie a key no tied form accepts is reported as unknown instead of "send only one of".

### Configurable note limits — 2026-11-26

- User request: raise the default note count to 20000 and make the note text, reply and author limits configurable through the environment, then deploy natively.
- Done on `feat/note-limits-env`: `NoteLimits` in scene-input (defaults, ceilings, messages) replaces the `MAX_NOTE*` constants; the server reads `FAGENT_MAX_NOTES`, `FAGENT_MAX_NOTE_TEXT`, `FAGENT_MAX_NOTE_REPLIES`, `FAGENT_MAX_NOTE_AUTHOR` once at startup (raise only, default..=ceiling). The `apply_batch` schema, bundle preflight and the note read reservation follow the configured limits. Docs: `SERVER.md`, `SERVER-OPERATIONS.md`, `BACKUP.md`, `NATIVE-RUNTIME.md`.
- Verified: scene-input, node-memory and storage tests, server startup/schema/context tests, clippy, wasm32 check; a debug server rejects `FAGENT_MAX_NOTES=100`, prints raised limits and publishes `maxLength` 8000 for note text in `tools/list`. `notes::a_notes_only_commit_enqueues_no_prerender` fails here on `main` too (no prerender in this environment).

### Viewer large-board load (Tasks 36/37 partial) — 2026-11-26

- User report: a large project's All pages board (~225 frames) takes over 7 s to render. Work on `perf/viewer-large-project` (from `feat/note-limits-env`); started on a loaded machine, measured and finished on a quiet M3 Pro.
- Decisions: paced progressive draws with `frame` messages held until the image that shows them (`drawPacing.ts`, `workerRuntime.ts`), `MessageChannel` yields between frames (`yieldTask.ts`) but timer polling for GPU waits (`pollTask`), per-font HarfRust shape plan cache (`crates/text/src/plans.rs`), cold-load benchmark `test:engine-load`. Item 2's "off-viewport commits do not dirty the board" dropped: pacing bounds the draws instead.
- Result: All pages `synced` 8.6 → 2.8 s on a synthetic 229-frame board; table, method and rerun commands in `docs/VIEWER.md` "Measurements — 2026-11-26". Committed in five pieces and pushed at the user's request as PR #8 (not merged).
- Not run: `test:react` (`app-shell-smoke.mjs` needs the pinned Chromium); `cargo test -p fagent-server` render tests need the Chromium render worker (same failure list on `77e2430`).
- Open: Task 36 items 3–4 and `--base-url`, Task 37 items 1 (native bench), 3–4, Task 35, parallel compute workers (needs a design decision).

### PR #8 review follow-up (Tasks 36/37) — 2026-11-27

- User request: pull `perf/viewer-large-project`, fix the CodeRabbit "Request changes" on PR #8 and report numbers. The local tree held an older uncommitted copy of the same patch; fast-forwarded to `origin` (backup patch kept outside the repo).
- Fixed: a failed last draw keeps held frames and `synced` until an image is drawn (`workerRuntime.ts`); the loader yields after fetching fonts, so a camera received meanwhile is drawn before that frame computes (`frameLoader.ts`; found by the new check, base included). Benchmark: integer counts with `pages ≤ frames` and counts taken from the generated fixture, `/snapshot` request mark, commit → `drawn` from the worker's commit, `cameraOrder` check. Commit, compute and camera times come from the glue wrapper instead of a new protocol event, so the product carries no probe. Docs: Task 36 `--scopes` commands, Task 37 attribution with cache-only evidence, the 2026-11-26 entry above shortened.
- Result: the plan cache alone cuts `compute` 45% on All pages; table in `docs/VIEWER.md` "Measurements — 2026-11-27".
- Verified: web tests 161 + 49, typecheck, `test:engine`, `test:engine-board --strict` (Chrome 158). No Rust change, so no cargo run.
- Committed in three pieces and pushed to PR #8 at the user's request. Open: the review threads are not answered on GitHub.

### Task 42 — Engine persistent scene cache — 2026-11-27

- User request: after PR #8 a ~225-frame board still takes ~6 s on a loaded machine; make later opens very fast through caching. WebGPU initialization (~3.8 s before the first frame on the user's machine) stays out of scope for now. Packaged as [Task 42](../../tasks/42-engine-persistent-scene-cache.md) (takes over the engine-path parts of Task 32 items 4–5; order 42 → 36 → 37 → 31/35 → 33).
- Built on `perf/engine-scene-cache`, then committed on `perf/viewer-large-project` at the user's request, pushed to PR #8 with the review fixes below. A test release of the uncommitted tree ran on port 3900 for the user to try.
- Decisions: records are `serde_json` with exact floats (no new dependency); imports wait for the backend like compute; restored frames skip the first-frame draw; item 5 done (worker Cache API keyed by font hash); the fixture's white fills became one token for `--edit-shared`.
- Result: the task file's status and `docs/VIEWER.md` "Measurements — 2026-11-27 (engine scene cache, Task 42)".
- Verified: web tests 161 + 61, `test:react` (on installed Chrome via a test-only preload; the pinned Chromium is absent), typecheck, `cargo test`/fmt/clippy for `fagent-engine-wasm` (wasm32 clippy: one older warning in `web/backend.rs`), `test:engine`, `test:engine-board --strict`, `test:engine-cache`.
- Open: `frame_keys` still blocks the attach on a changed ETag (Task 35); Safari/Firefox not measured; the browser HTTP cache is off in all benchmark runs (Playwright routing).

### Scene cache code review fixes (PR #8) — 2026-11-27

- Assigned from a code review of `perf/viewer-large-project` against `main`; fixed on the same branch, committed in two pieces and pushed to PR #8 at the user's request.
- Fixed (behavior in `docs/VIEWER.md` "Engine scene cache"): a new ETag at the same revision resends the snapshot, and the worker trusts only the ETag that arrived with its loaded snapshot (digests of older bytes were filed under the new ETag); records touched in a write batch rank as used before it evicts and evicted rows get no usage row; other builds are swept once per worker, not on every write; a sync superseded during its manifest read runs no `frame_keys`; damaged manifests are misses; manifest hits refresh their last use; oversized records are counted apart from writes; the font cache is bounded to 128 MiB too.
- Verified: web tests 161 + 67, typecheck, `test:engine-cache` and `test:engine` (installed Chrome; the pinned Chromium is absent). The new tests fail on the previous code. No Rust change.
- Open: without a strong ETag, a 200 with new bytes at the same revision still leaves the worker on the older snapshot (unchanged behavior, outside the cache).

### CodeRabbit reviews 2–3 on PR #8 — 2026-11-27

- User request: fix every open CodeRabbit finding on PR #8 (reviews on `66d82f4` and `8ac57df`, 13 findings) and the failed "Task Acceptance Criteria" pre-merge check. Review 1 (`5cb67b2`) was already fixed in `bc9e3fd`..`66d82f4`.
- Fixed (behavior in `docs/VIEWER.md`): a reused frame waits with an older sync's held message for it; a restored-only release no longer delays the first computed frame; font failures of a superseded sync, or of a font only a stored record lists, are not remembered; the font cache retries a failed open and frees space on a quota error; records that repeat a node id are rejected (Rust); the load probe accepts the last image before `synced` and pairs each commit with its own image; warm edit rounds count across scopes (unique keys and whites); the browser test stops Vite when the browser cannot launch; the ETag client test compares full pairs; the 250 ms wording. Client tests moved to `engine-client.test.ts` (the runtime test file had passed 1,000 lines).
- Pre-merge check: "PR scope" lines in tasks 36, 37 and 42 (one PR at the user's request, deferrals listed); the PR description must still name Tasks 36, 37 and 42 (no GitHub CLI on this machine).
- Verified: web tests 161 + 71, typecheck, `test:engine-cache`, `test:engine`, `test:engine-board --strict` (installed Chrome, rebuilt engine module), `cargo test`/fmt/clippy for `fagent-engine-wasm` (wasm32 clippy: older warnings only), `engine-load.mjs` smoke runs (20 frames, two scopes: cold, `--warm --edit 1`, `--warm --edit-shared`). New tests fail on the previous code.
- Committed in four pieces and pushed to PR #8 at the user's request. Open: the GitHub review threads are not answered; the PR description does not name Task 42 yet.

### Agent feedback triage (Tasks 43–46) — 2026-11-27

- User request: turn the maintainer's private agent-feedback log (52 items) into a fix plan, then into one task per phase for other sessions. Read-only trace against `main` at `7d71753`, whose server crates equal the deployed build.
- Packaged [Task 43](../../tasks/43-feedback-phase1-small-fixes.md) (small fixes; four slices with disjoint files), [Task 44](../../tasks/44-feedback-phase2-instance-overrides.md) (deeper instance overrides, context size; after 43 slice B), [Task 45](../../tasks/45-feedback-phase3-semantic-changes.md) (behavior changes waiting for user decisions) and [Task 46](../../tasks/46-feedback-phase4-feature-backlog.md) (feature backlog index). Items are named `FB-n` after the log's numbering; examples are synthetic.
- Already fixed on `main`: FB-3, FB-4, FB-6, FB-7, FB-9 (b, c), FB-11, FB-20, most of FB-8. Not reproduced from code: FB-1, FB-27 (Task 43 reproduces them first).
- Supersedes the open layout and instance triage in "Agent feedback: known limitations — 2026-11-17" (its items are in Tasks 43–46).
- Replaced a private task name in Task 39 with a generic description.
- Open: nothing assigned; branch `fix/agent-feedback` holds the task files.

### Task 43 (agent feedback, phase 1) — 2026-11-27

- User request: implement [Task 43](../../tasks/43-feedback-phase1-small-fixes.md); four subagents built slices A–D in parallel worktrees (`fix/feedback-ocd`, `-instances`, `-reads`, `-storage`); the lead reviewed each and merged them into `fix/agent-feedback` (not pushed).
- Result: every item fixed, message-only or documented as the task asks; FB-27 not reproducible (pinned by tests); FB-14 traced to the client (the server sends U+E40D intact). Per-item table in the task's "Result".
- Decisions: every `free` container is `Position::Relative` (no golden moved); `render_frame` over area returns `PIXEL_BUDGET`; new cursor format; discovery-workflow budget raised to 105,000 B (user); new tool texts trimmed to keep `tools/list` within 32 KiB.
- Verified: crate tests show only the 41 failures already on `main` (Chromium worker, prerender note test, two `startup_env` PostgreSQL tests); clippy, fmt, public-content check; PostgreSQL ignored tests through `scripts/task19-check.sh`.
- Open: Chromium-path checks not run end to end (no worker in tests); font override keys deferred; Tasks 44 and 45 can start (44 depends on slice B, now merged here).
- Self-review: fixed `inspect_layout` overflow paging (a huge `diagnosticsOffset` overflowed; a byte budget made `nextOffset` skip dropped items); other review findings checked and left as designed.
- PR review (CodeRabbit, 6 findings + docs check): all fixed. Capabilities publish `overrideKeys`; enqueue/paint admission and the hug-root `maxHeight` use whole device pixels; bare `clip` is rejected on `@token`; read cursor scopes end with a 128-bit digest of all bound fields (golden cursors regenerated, no page boundary moved); an overflow page the budget empties keeps `nextOffset` and sets `budgetTooSmall`; `deletedNoteIds` documented in SERVER/CONTRACT.

### Task 47 (native desktop app) — 2026-11-28

- Packaged [Task 47](../../tasks/47-native-desktop-app.md) at the user's request: `fagent-server` stays the headless binary; new `fagent-desktop` runs the viewer with an optional in-process server sharing one `Storage`/`NodeMemory`. Cases C0–C5 (default: local SQLite, MCP off), settings with port/database probes, remote-server mode, tray.
- Phase 0 is a decision gate (native `vello_gpu` + UI toolkit choice).
- Revised after a code check and user decisions: SQLite `.instance.lock` makes a shared `~/.fagent` one-process-at-a-time (locked-root flow added); PostgreSQL probe must not call `with_config` (it initializes the schema); a port change rebuilds `AppState` (`base_url` is fixed per `Config`); live updates are in-process only; tray hides and freezes state; tabs; External mode includes loopback servers; every MCP change restarts `AppState`; PostgreSQL is user-managed (no cross-process live updates).

### Viewer lag on large boards (investigation) — 2026-11-28

- User question: why zoom/pan still lags on a large project although LOD exists. Read-only; measured with `fagentEngine.status().lastRender` on a 399-frame, ~46k-node board (WebGPU, DPR 2).
- Result: draw cost grows with the nodes in view, not with pixels. At fit (device scale ~0.007, a frame ~10 device px wide) every draw encodes ~46k nodes, ~12.5k clip/opacity layers and ~24k greeked bars: encode 20–41 ms + submit 5–13 ms (~45 ms p50), so the image updates at ~20 fps and a pan past the 2.5% overscan shows blank strips. At scale ≥0.3 a draw is ≤3 ms.
- Causes: no size-based culling (subtrees below a device pixel are still encoded), clip/image layers pushed regardless of size, the whole board re-encoded on every camera change (no per-frame thumbnail or tile cache in the engine). Not measured: GPU raster time, main-thread overlay (pane hidden).

### Task 44 (agent feedback, phase 2) — 2026-11-28

- User request: implement [Task 44](../../tasks/44-feedback-phase2-instance-overrides.md) on `fix/feedback-overrides` from `main` at `2983fd4`; item 5 by a subagent on `fix/feedback-context-size`.
- Decisions (user): 1 layout overrides merge per key, `breakpoints` replace whole, no `x`/`y`; 2 no `overrides.<masterRootId>` alias, and fix the instructions that led agents to write it; 3 (a) cross-axis `fill` under `align:"stretch"`; 4 `@set overrides={…}` merges per descendant; 5 depth-limited components paged as cursor entries.
- Item 2 cause found in the agent texts: the component contract said an instance's own keys cover only name, visibility, position and sizing (paint omitted), the server instructions said "flat overrides" without naming the root, and no example set a root prop on an instance. All three now say root props go on the instance next to `componentId`.

### Large-board zoom and pan follow-up — 2026-11-28

- Confirmed the redraw path in `draw.rs` and `encode.rs` against the earlier viewer lag measurements. The recorded sample has no project identifier, so its timings cannot be attributed to the reported project.
- Result: every item fixed; per-item table in the task's "Result". `components` in `get_design_context` is now a paged array (breaking; no viewer code read it).
- Verified: crate tests show 41 failures, all also failing on `main` at `2983fd4` (Chromium render worker, the prerender note test, PostgreSQL/listener `startup_env` tests); the seven new Task 44 tests and the item 5 tests fail on `main`; clippy `-D warnings`, fmt, public-content check.
- Open: the `get_design_context` tool text does not describe `components` paging (`tools/list` 32 KiB cap); the `instances_mcp` paging assert after a render step does not run without Chromium; not pushed.
