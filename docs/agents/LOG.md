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
- Vietnamese test strings and fixtures were replaced with English (accented Latin where a test needs diacritics); Chromium-measured corpus entries in Vietnamese were removed rather than translated.
- New rules in `AGENTS.md`: English-only content and a blocking "Public repository" rule. `scripts/check-public-content.sh` (run by `.githooks/pre-commit`) blocks Vietnamese text, local absolute paths, real tailnet hosts, secrets, non-example emails and the names in the untracked `.private-patterns`.
- Open-source scrub: client project names, IDs, slugs and snapshot hashes, real backup names, a real tailnet host and the migration scripts for the previous product were removed; machine notes moved to the untracked `CLAUDE.local.md`. Limitation IDs that pointed to the previous product's feedback list were replaced with plain descriptions.
- The README screenshots and motion GIF were removed because the demo frame's footer showed the previous product name. Open: the user will add new screenshots to `README.md`.

### Task 40 — Liquid glass shader — 2026-11-18

- Packaged [Task 40](../../tasks/40-liquid-glass-shader.md) (backlog) at the user's request: backdrop blur and a glass material (refraction, chroma, specular) through a custom WGSL pass plus a `vello_cpu` reference, because Vello 0.4 only executes Flood, GaussianBlur, Offset and DropShadow filters.
- Open: not assigned; phase 0 spike decides segmented rendering vs prefix re-encode and the WebGL2 path.

### README redesign and screenshots — 2026-11-18

- README restructured after well-known open-source READMEs: hero GIF, tagline, quick links, gallery, one-line highlights, short quick start, agent connection, performance summary. Reference sections moved verbatim to `docs/GETTING-STARTED.md`, `docs/ARCHITECTURE.md` and `docs/PERFORMANCE.md` with relative links fixed.
- Closes the open screenshot item above: `docs/assets/readme/` holds `viewer-frame.png`, `all-pages-board.png`, `viewer-inspector.png`, `motion-preview.gif` and `motion-orbit.gif`, captured from a demo project authored through MCP.

### Task 41 — Node-memory engine — 2026-11-18

- Packaged [Task 41](../../tasks/41-node-memory-engine.md) at the user's request after the commit-path review: one self-contained engine struct in `crates/node-memory` with no database knowledge, always on (no `FAGENT_NODE_MEMORY_*` flags or modes), full validation once at load (warm | cold), diff-only validation per commit with per-op checks kept, error order among end-of-batch errors allowed to change, no pre-expanded instances.
- Open: not assigned; admission behavior for invalid stored data to confirm in phase 2.
- Assigned 2026-11-18 on `feat/task-41-node-memory-engine`; phase 1 (boundary) started. Decisions: struct name `NodeMemory`; admission on invalid stored data = serve reads, keep diff-only commit validation, count and log with project/node ids only (implemented in phase 2); fix pre-existing clippy errors in storage/server in a separate commit; spike 1b (single persistent state) waits for user approval after phase 1.
- Phase 1 done 2026-11-18 (`fe03415..6cc6c67` plus docs): `NodeMemory` always on, flags/shadow/modes removed, SQLite and PostgreSQL commits stage in the engine; differential 3,600 batches with 0 differences; small props edit 36.5 ms on PG 200k (former memory path 55.9, database path 988, back to back). Report: [TASK41-REPORT.md](../TASK41-REPORT.md). Benchmarks on this machine need a low load average; under load the same code measured 2–3× slower.
- User approved 2026-11-18: spike 1b runs next in its own session; `imbl` (MPL-2.0) may be added as a dependency. Phase 2 waits for the spike decision.
- Spike 1b done 2026-11-18 on `spike/task-41-1b` (measurement only, engine unchanged; `imbl` is a bench dev-dependency only). On the 200k fixture (101 runs, two rounds, load ~2) one persistent state beats arena + `ReadView` on every row: RAM 1,563–1,722 vs 3,280 B/node, small commit 1.0–1.3 µs vs 20 µs, frame read 22–60 vs 91 µs. Recommendation: hand-written radix trie (no dependency). Finding: `ReadView` publication is superlinear (14k-sibling renumber ~9 s, 328-node delete ~210 ms under the write lock). Details: [TASK41-REPORT.md](../TASK41-REPORT.md) "Spike 1b". Open: user decides the structure before phase 2.

### Task 41 phase 2 — 2026-11-18

- User decision (c): one persistent `ProjectState` over a hand-written radix trie replaces the slot arena, undo log and separately published `ReadView` (publish was superlinear: 14k-sibling renumber ~9 s, 328-node root delete ~210 ms under the write lock). Reject drops the clone; commit publishes the validation preview.
- Done on `feat/task-41-phase-2`, `10a3727..c649057` plus docs: validator `Lookup` trait, per-op property checks in `Txn` (exact vs routed rejections), tokens validated once per batch, admission at load (invalid data served, counted, logged with ids/codes only). Differential 12,000 `begin` batches, 0 differences. 200k: 1,594 B/node, wide-frame renumber 5 ms, root delete 0.5 ms; PG small edit 31.8 ms (phase 1 36.5); admission ~0.9 s per load, first commit 27 ms. Report: [TASK41-REPORT.md](../TASK41-REPORT.md) "Phase 2".
- Open: stored dangling token alias or broken `componentId` blocks every commit until repaired (user decision on tolerating defects); token batches O(ops × tokens) on the reference path; admission cost per load. Phase 3 not started.
