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
