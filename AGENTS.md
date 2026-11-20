# FAgent — working rules for agents

This file is loaded into every session (Codex, Claude Code), so it holds only rules, current status and references.
The active log is `docs/agents/LOG.md`; rejected technologies are listed in `docs/REJECTED-APPROACHES.md`.

## Working rules

- Read `PLAN.md` before implementing. It is the source of product requirements and AC.
- Packaged tasks waiting to be assigned live in `tasks/`; read them before defining new work. Each file is a complete handoff, not a note.
- Codex: hand each implementation task to a new subsession (separate chat); the main session only coordinates and accepts, and workers do not start further sessions. Claude Code keeps using subagents.
- Handoff: goal, scope, workspace/branch, AC, how to verify, outputs and authority. Codex uses `create_thread`; do not fork history or change model/effort on your own; do not reuse finished sessions.
- Use `rg`/`rg --files` and read through the shell; use `apply_patch` to edit code. Do not use Python for routine text operations. Exclude node_modules, .git, .venv, dist, build from searches.
- Prefer explicit RTK where supported (`rtk ls`, `rtk git status`); use raw output for diffs, debugging and evidence. Do not enable hooks/proxy/experimental features on your own. See `~/.codex/RTK.md`.
- Codex: while a worker runs, wait only with `wait_threads`; do not read/edit/review source or comment midway; accept after handoff. Handle user steering when it arrives.
- Verify exactly the changed area; record a pass only after running it. Codex may create and send handoffs and follow-ups to implementation subsessions; Claude Code keeps its previous approach.
- Version only source, tests, conclusion reports and rerun commands; generated benchmark logs/JSON/JSONL/manifests/archives must be ignored, never committed. See `spikes/realtime-db/evidence/README.md`.
- Do not write demo data into `~/.fagent`; tests use a temporary data root. Only a real create_project API call initializes a user project.
- Write to `docs/agents/LOG.md` at each assignment, result, blocker found and before stopping (format under "Log and memory"). Never put secrets or the user's private data paths into the API.
- **English only (user rule, 2026-11-17).** Everything committed is in English: docs, tasks, logs, reports, code comments, test strings, fixtures, sample content and commit messages. Talking to the user in another language is fine; repository content is not.
- **Public repository — no private data (user rule, 2026-11-17, blocking).** This repo is open source. Never write into it: real project names, IDs, slugs, frame/node IDs or snapshot hashes from the user's workspace; names of clients, other products or private repos and their paths; people's names, usernames, emails, machine or Tailscale host names, private IPs; local absolute paths (`/Users/<name>/`, `/home/<name>/`, `/Volumes/`, `/var/folders/`); backup names, credential locations, other systems' database names; secrets. Use synthetic fixtures, placeholders (`<repo root>`, `$HOME`, `$TMPDIR`) and aggregate numbers ("a 7k-node project"). Machine notes go in the untracked `CLAUDE.local.md`; names to block go in the untracked `.private-patterns`. Run `scripts/check-public-content.sh` before finishing; the pre-commit hook (`git config core.hooksPath .githooks`) runs it (plus gitleaks via `.gitleaks.toml` when installed) on staged files and blocks the commit. Never bypass it (`--no-verify`) or weaken it without the user's approval.
- Before adding a dependency, engine, renderer or storage design, read `docs/REJECTED-APPROACHES.md`; do not reintroduce a rejected approach without the user's approval.

## Modular Architecture Rules — user update 2026-11-16

- Prefer modules with clear responsibilities; follow the existing architecture, with small, explicit public APIs.
- Before a feature: survey the structure, define responsibility boundaries and the files to create/change; split when it makes the code clearer, and keep related logic together.
- Over 500 lines: consider splitting, not mandatory. Over 1000 lines: needs a concrete reason. Line count is not a hard acceptance limit.
- Avoid creating abstractions or small files only to hit a line count.
- Move business logic into services, reusable functions into utilities, types/interfaces where useful, UI pieces into components and stateful UI logic into suitable hooks.
- Refactor when responsibilities are mixed or the structure is hard to follow; keep behavior and public interfaces, avoid circular dependencies; update imports/tests after splitting.
- Acceptance: review responsibilities and sizes; explain any file over 1000 lines; run the relevant checks.
- Applies to every handoff. The user's original is in the shared memory (`$CLAUDE_CONFIG_DIR/CLAUDE.md` for Claude Code, the global instructions for Codex).

## Log and memory

- `AGENTS.md` holds only rules, current status and references. Budget ≤120 lines / ≤10 KB; check `wc -l -c AGENTS.md` before ending a session.
- "Current status" is at most ~10 lines and is **replaced** when the status changes, not appended to. No PIDs in `AGENTS.md`.
- Assignments, results, blockers and stop points go into `docs/agents/LOG.md` (append-only): one dated heading per task (`### <task> — YYYY-MM-DD`), a few lines per write, only decisions, final results, blockers and report links.
- Do not copy content already in `docs/TASK*-REPORT.md`, `tasks/`, test output or git history; no intermediate numbers. Long technical detail belongs in the task's report/docs, not the log.
- When `LOG.md` passes ~300 lines: delete closed entries. If a closed entry records a technology or approach that was tried and dropped, first add one line for it to `docs/REJECTED-APPROACHES.md`. Closed task files and reports may be deleted the same way.
- Memory outside the repo (the Claude Code memory folder, `~/.codex/memories`, global CLAUDE.md/AGENTS.md files) stores only pointers or facts needed across sessions; never copy the repo log there.

## Current status — 2026-11-19

- FAgent is a standalone project; technical identifiers use the `fagent` prefix (renamed throughout on 2026-11-17, no compatibility with old names). Git history was restarted on 2026-11-17 as an English-only repository on `main`. The new remote is set by the user. No FAgent deployment or runtime yet.
- Codebase: scene-3/web-4, MCP-first, storage SQLite by default or PostgreSQL (`DATABASE_PROVIDER`/`DATABASE_URL`). `NodeMemory` serves every latest read and batch (validation per load, diff-only batches, no snapshot cache; Task 41 done on `feat/node-memory-optimization`, unmerged; `docs/TASK41-REPORT.md`); deep checks: `docs/FAGENT-BASE-VERIFICATION.md`.
- Main direction: GPU viewer through Vello (`vello_gpu`: WebGPU first, WebGL2 fallback; `vello_cpu` for export/fallback). See `docs/VELLO-GPU-RENDERER-RESEARCH.md`, tasks 29–34. Task 29 (P0): conditional GO, measurements under "P0 results". Task 30 (P1) accepted: Worker engine; since 2026-11-17 Vello is the default (WebGPU first), `?renderer=canvas2d`/`?scene=server` keep Canvas2D; see `docs/VIEWER.md` "Vello engine". Task 34: phases 1–3 done, phase 4 partly done (export backend chosen at build time with `FAGENT_RENDER_BACKEND`); see the task file. Next: 36 (cold load), 37 (shape plan/paint caches), then 31/35/33. Open: `test:e2e`/`test:zoom` harnesses already broken; Safari 26 and iOS not measured; Layers panel virtualized (the ~90 ms long task not re-measured).
- Machine-specific notes (other services, databases and credentials on the maintainer's machine) live in the untracked `CLAUDE.local.md`; read it at session start when it exists. Never connect to or write into services or databases you did not start; tests use temporary data roots and databases. Do not write the real `~/.fagent`.
- Do not reset/checkout/stash or discard other people's changes; push only when the user asks.
- On a new request: read this section, the open entries in `docs/agents/LOG.md`, README/ACCEPTANCE and `git status`.

## References

| Path | Read when |
| --- | --- |
| `PLAN.md` | Before any implementation: product requirements and AC. |
| `README.md`, `docs/GETTING-STARTED.md` | Setting up, running the app or connecting MCP (README is the overview; the guide has the details). |
| `docs/ARCHITECTURE.md`, `docs/PERFORMANCE.md` | Crates and render/export backends; recorded benchmarks. |
| `docs/ACCEPTANCE.md` | Accepting work; you need AC, check commands or published limits. |
| `docs/REJECTED-APPROACHES.md` | Before adding a dependency, engine, renderer or storage design. |
| `tasks/` | Before defining new work. 20/20b: local Kubernetes and public viewer; 21: text ink rounding; 22: fewer tokens per design session (inbox import, compact render, props by kind, compact reads); 23: implementation vs design conformance (expected spec + DOM measurements, expected/actual table); 24: large-project benchmark; 25: incremental validation; 26: compact authoring DSL; 27: native deploy with process-compose; 28: node memory; 29–33: Vello GPU viewer; 34: Vello export, dropping Chromium (first); 35: frame-scoped resolve; 36: fast cold load (web); 37: shape plan/paint caches (Rust); 38: Kubernetes smoke for `liveUpdates` (low priority, backlog); 39: render past revisions (backlog); 40: liquid glass shader (backlog); 41: node-memory engine. Order: 34 → 36 → 37 → 31/35 → 33. Status/AC at the top of each file. |
| `docs/TASK*-REPORT.md` | You need technical detail, measurements or open risks of a task. |
| `docs/SERVER.md` | Changing or calling the backend: MCP, HTTP API, jobs, SSE. |
| `docs/CONTRACT.md` | Integrating modules (viewer ↔ HTTP API, data types). |
| `docs/VIEWER.md` | Changing the `web/` viewer or the shared renderer. |
| `docs/RESOLUTION.md` | Touching tokens, aliases, themes or the M3 resolver. |
| `docs/BACKUP.md` | Touching migrations or M4 export/import/restore bundles. |
| `docs/NATIVE-RUNTIME.md`, `docs/NATIVE-BACKUP.md` | Operating native macOS, PostgreSQL 2703, launchd, backup and recovery. |
| `spikes/e2e/README.md` | Running or fixing the real E2E (`npm run test:e2e`). |
| `docs/agents/LOG.md` | At session start (open entries) and at every log write. |
