# Task28 — Node-memory integration (HOT projects, per-property LWW)

**Status:** I0 (`8ebadc5`..`2a50699`), I1 (`354ec41`..`5d6f08d`), I2 (`383cb9e`..`36897b3`) and I3 (`7b756c4`..`f975d2d`) done and verified 2026-11-16. I3b (`3872f26`, `cc45e55`), I4 and I5 code implemented on `feat/node-memory-writes` with bounded checks; deep tests and all benchmarks remain deferred by the user. The two previously blocked I5 guard/consumer patches were specifically approved by the user and applied through normal review. No latency/RAM or full PostgreSQL parity claim; optional N7 packing remains unimplemented. See `docs/TASK28-I3B-REPORT.md`, `docs/TASK28-I4-REPORT.md` and `docs/TASK28-I5-REPORT.md`.
Requested by the user on 2026-11-16 after the 200k/5M benchmarks of
`crates/node-memory` (branch `feat/node-memory`).

## Why

Every read and commit loads the whole project from the database, clones it and rebuilds
indexes per revision. PG 30× (scoped, median): load 378 ms, `from_snapshot` 123 ms,
validate 1,061 ms (instance expansion 680 ms), `persist.delta` 110 ms, cache fill 104 ms;
`get_design_context` on one frame 403 ms. `crates/node-memory` keeps a project in RAM
with indexes updated per operation: at 200k nodes a frame subtree is 0.6–27 µs, a props
update 0.4 µs, a reparent 1.5 µs; about 1.3 KB RAM per authored node (two real projects
measured 2026-11-16: 6,711 + 6,356 authored nodes ≈ 17 MB; largest sibling list 60, median 2).

## Decisions (user, 2026-11-16)

- In-process: `node-memory` is used by `crates/storage`; the database stays the source
  of truth. Every commit writes its delta synchronously before ACK (no debounce queue).
- HOT/COLD registry with TTL and an LRU byte budget; projects load on first access.
- Conflict model like Figma: one writer per project orders commits; **last writer wins
  per property** (a top-level props key; `merge:true` keeps deep merge). `(parentId,
  sortKey)` is one combined property. Revision stays as a sequence number (history,
  `get_changes`, SSE cursor, idempotency) but stops being a precondition from I3 on.
  Only invariant violations reject: cycles, missing/deleted parent, component in use,
  schema/validation errors, virtual-child writes.
- Agents get cheap feedback instead of retries: optional `baseRevision` → the commit
  succeeds and reports `concurrent` (≤20 node/key pairs others changed after
  `baseRevision` that this batch overwrote). Errors name enough context to fix without a
  re-read. `expectedRevision` + `strict:true` keeps today's behaviour for those who ask.

## Slices

| Slice | Content | Behaviour change | Target |
| --- | --- | --- | --- |
| I0 | Complete the lib (below) | none | prerequisite |
| I1 | Shadow: mirror every committed batch into RAM, compare rows with the DB | none | correctness on real data |
| I2 | Reads (`get_design_context`, `get_subtree`, `find_nodes`, `get_outline`, `list_*`) from a scoped slice in RAM, reusing today's readers | faster reads | frame read 2–4 ms |
| I3 | Commit through RAM: no `entities_for_write`, delta from the lib; LWW semantics, optional `expectedRevision` + `concurrent`, `strict` opt-in; MCP docs/skills updated | writes | −~715 ms, no conflict retries |
| I3b | Persistent reader-format indexes per HOT project, updated from each commit's `Delta` and published per revision with structural sharing; no slice copy | faster reads | no per-read index rebuild; real-project read at or below cached path |
| I4 | Incremental validation: affected scope via `instances_of`/`master_of`, expand only affected instances | faster writes | small edit 2–5 ms; master edits scale with instances |
| I5 | Remove `storage/src/scoped/` guards, Moka snapshot cache and `RevisionIndexes` for HOT projects; reader rewrite on persistent indexes; typed props (N7) | cleanup | read < 1 ms, less RAM |

Each slice: own worker, branch from `feat/node-memory` (or its successor), small commits,
no push/deploy/restart, no writes to `~/.fagent`, ports 3857/3858/3877 or PG 2703.

Module policy (user, 2026-11-16): prioritize clear responsibilities; above 500 lines
consider splitting without making it mandatory; above 1000 lines explain why the file
stays together. Avoid abstractions or small files solely for a line-count target.
This supersedes all earlier per-slice 250-line limits below for future work.

## I0 — complete the lib (done)

Workspace: the separate worktree `FAgent-node-memory`, branch `feat/node-memory`.
Only `crates/node-memory` (plus workspace manifests if a dependency is added).

1. **Two-phase transactions.** `ProjectHandle::begin(&ops) -> Result<Staged>`: ops are
   applied under the write lock with the undo log open; `Staged` exposes the `Delta`
   and read access to the staged state; `Staged::commit()` keeps it, dropping or
   `abort()` rolls back exactly. `apply` stays as `begin` + `commit`. Readers never see
   staged state (they wait on the lock while a batch is staged).
2. **Existing `Operation` input.** Accept `fagent_scene_input::Operation` directly
   (CreateNode/UpdateNode/ReparentNode/ReorderNode/DeleteSubtree with `index`, and
   UpsertToken/UpsertStyle/CreateNote/ReplyNote/UpdateNote/DeleteNote). Node position
   must reproduce `Candidate::place` exactly (`scene-input/src/validate/candidate.rs`:
   siblings sorted by `(sort_key, id)`, inserted at `index` or appended, then renumbered
   0..n with changed rows reported), using the children index instead of a project scan.
   `UpdateNode` follows the same `extend` vs `merge_props` rule as `Candidate`.
3. **Resources in RAM.** Tokens, styles, notes (and fonts/assets metadata) live in the
   state with the nodes; their ops update them under the same transaction; deleting a
   subtree handles notes the same way the current storage does (check `scoped/notes.rs`
   and the batch code). `to_snapshot` round-trips everything.
4. **Equivalence oracle.** A differential test: random batches of real `Operation`s
   applied to the lib and to `Candidate::from_snapshot(..)` + its apply path; the
   resulting `Snapshot` (nodes, sort keys, resources, notes) must be identical, and the
   lib's changed-row set must equal the candidate's changed ids. Rejections must agree
   for structural errors (the lib may accept what only the full validator rejects; list
   those cases explicitly).
5. **Scoped slice.** `ProjectState::slice(root_id) -> Snapshot`: the root frame's
   subtree plus the dependency closure the current scoped readers need (masters reached
   through instances, recursively; ancestors up to the page; tokens/styles/fonts/assets;
   notes anchored in the slice). Verify against `read_scope` in scene-input: running the
   existing scoped `get_design_context` path on the slice gives the same response as on
   the full snapshot for every root frame of the 5k fixture.
6. **LWW groundwork (no API change yet).** Record per node and per top-level props key
   the revision that last wrote it, so I3 can compute `concurrent` for a `baseRevision`.
   Keep it compact (measure RAM at 200k; report bytes/node added).

**AC:** fmt, `clippy --all-targets -D warnings`, `cargo test --release -p
fagent-node-memory -- --include-ignored` pass; the 200k bench shows no regression
and reports the new ops (begin/commit, `index` placement in the 14k-child frame, slice of
a normal root frame); files ≤ 250 lines. Never print or read generated fixtures into an
agent context: counts and timings only.

**Output:** commits, API summary, equivalence cases found (with how they were resolved),
bench table, RAM delta, files over 200 lines, open questions for I1.

### I0 result (verified by the root session)

44/44 release tests incl. the 200k fixture; oracle vs `Candidate` (8 seeds × 300 batches)
matches rows, changed ids and errors. Lib-only differences are all rejected by full
validation (component unmarked while used, depth >128 through descendants, node cap at
create). The lib does not run property/token/style validators and does not enforce
`MAX_NOTES`. 200k bench: update 584 ns, begin+commit 542 ns, slice of a normal root
frame 98 µs, insert at index 0 of the 14k-child frame 1.45 ms (Candidate renumbering),
LWW stamps +24 B per written node. `read_scoped` is `pub(super)`, so the slice was
checked at the `read_scope` layer.

## I1 — shadow mirror (done)

Branch `feat/node-memory-shadow` from `feat/node-memory` (same worktree). Behaviour of
every API stays identical; the shadow can only log and count.

1. `crates/storage` depends on `fagent-node-memory`. A `Shadow` wrapper (own module)
   holds a `Registry` and is driven by both backends (SQLite and PG) after the DB
   transaction commits: apply the committed batch's operations to the HOT project
   (loading it from the snapshot already in hand on first touch), then compare.
2. Compare: revision, changed ids and the persisted delta rows as sets keyed by
   (kind, id) with canonical JSON (DB order differs from `to_snapshot`). Tests also
   compare the full row set after each batch. Any mismatch: count + structured log with
   project, revision and the first differing ids, then evict that project (next touch
   reloads). Never fail or slow the request on shadow errors.
3. Writes outside batches (asset/font import, project meta update, bundle import/restore,
   migrations, retention): evict the project (reload on next touch). No new ops yet.
4. Never hold the `std::sync` lock across `.await`: do the shadow work in one sync
   block (or `spawn_blocking` if measurable).
5. Opt-in by config/env (`FAGENT_NODE_MEMORY_SHADOW=1` or the storage config struct),
   off by default; on in the storage/server test suites run for this slice.
6. Real-data check without touching the live system: restore a copy of the latest native
   backup into a temporary cluster/data root (never PG 2703, never `~/.fagent`,
   never the live backup files in place), run a scripted series of batches built from
   real node ids through the storage API with the shadow on, zero mismatches. If that
   is not feasible, say why and what was done instead.

Decided defaults for I0's open questions: compare as sets by id (Q2); evict on non-batch
writes (Q1); no lock across await (Q3). Deferred to I3: history fallback for an old
`baseRevision` (Q4), stamps for renumbered siblings (Q5), dense renumbering vs gapped
sort keys (Q6), validating staged state (Q7). Q8 (borrowed slice) is I5; Q9 (`MAX_NOTES`)
can be added in I1 if trivial.

**AC:** shadow off ⇒ no behaviour change; shadow on ⇒ storage + server test suites
(SQLite, and PG where the suite already runs on a temp cluster) pass with zero
mismatches; real-data check reported; fmt/clippy clean for touched crates; files ≤ 250
lines; overhead of the shadow per commit measured (median, 200k fixture and one real
project size).

User decision 2026-11-16 for Q6: keep dense renumbering 0..n (Candidate semantics). Real
sibling lists are small (largest 60 and 24 in the two measured projects), so it is
not critical; revisit only if very wide lists appear.

### I1 result (verified by the root session)

`feat/node-memory-shadow`: `crates/storage/src/shadow{.rs,/}`; flag
`FAGENT_NODE_MEMORY_SHADOW` = off (default) | `1`/`on` | `strict` (tests: full-row
compare, panic on mismatch). SQLite hook in `Project::run_batch` under the writer lock;
PG hook after `pool.run`, before cache fill; per commit: load/resync/skip, `begin`,
compare (revision, changed ids, touched rows as canonical JSON by (kind,id)), commit or
roll back + evict. Evicts on asset/font import, privacy update and `open_project`.
Storage tests 200/200 off and strict; PG suites strict 37/37 (temp Docker); server
335 pass / 36 env failures identical off and strict (render worker, Chromium, Node module).
Real data: a recent native backup restored to a temp cluster, 2,361
commits matched, 0 mismatches. Overhead: 17–26 µs per commit on real projects, 114 µs on
the 200k fixture; first-touch load 2.5 ms real, 76 ms at 200k.

## I2 — reads from RAM (done)

Branch `feat/node-memory-reads` from `feat/node-memory-shadow` (same worktree).

1. One `Registry` per storage instance, shared by the mirror and the readers. New flag
   `FAGENT_NODE_MEMORY_READS` (off by default); reads on implies the mirror on (the
   mirror is what keeps RAM current). Shadow compare stays as today.
2. Scoped reads (`get_design_context`, `get_subtree`, `find_nodes`/`get_outline` with a
   frame/node/page scope, `inspect_layout`, `get_edit_context`): build `slice(root)` from
   RAM and run the existing readers on it. Project-wide reads and explicit historical
   `revision` reads keep today's path in I2.
3. Freshness before serving RAM: the RAM revision must equal the DB's current revision
   (PG: one indexed query on `projects`; SQLite: in-process state). Behind ⇒ reload or
   fall back to today's path; count both.
4. First touch loads synchronously from the snapshot today's path would load anyway.
5. Assets/fonts accessors in the lib if readers need them (slice copies them).
6. Equivalence: responses must be byte-identical to today's path. Differential test on
   the 5k fixture (every root frame, nested frames, instances, virtual ids, paging
   cursors) and on the real-data restore (temp cluster, same rules as I1).
7. Counters: RAM hits, fallbacks, reloads (extend the shadow stats).

**AC:** reads off ⇒ no change; reads on ⇒ differential identical, storage + server suites
pass (server: the same 36 known env failures, no new ones), PG suites via
`scripts/task19-check.sh`; timings for a frame `get_design_context` (200k fixture and a
real project): today cold, today cached, RAM — target 2–4 ms; files ≤ 250 lines; no
deploy/restart, nothing on PG 2703 or the live ports.

### I2 result (verified by the root session)

`feat/node-memory-reads`: `StorageBackend::scoped_snapshot` (default = today's path;
SQLite/PG override via `Shadow::scoped_read`), flag `FAGENT_NODE_MEMORY_READS` =
off | on | strict (tests: run both paths, panic on any byte difference). RAM serves only
when its revision, incarnation, schema and asset readiness match the backend's existing
cache-identity probe; otherwise today's path plus load/reload. Through RAM:
`get_design_context` (frameId), `get_subtree` (nodeId, virtual ids), `get_outline` and
`find_nodes` with a frame/node/page scope. Still today's path: project-wide reads,
`list_*`, explicit `revision`, `viewports`, `get_edit_context` (page-wide closure),
`inspect_layout`. Differential: 0 differences over 130 (committed fixture), 2,199 (5k)
and 20,238 (real backup restore, temp cluster) reads with interleaved writes. Storage
206/206 off and strict; server same 36 env failures off and strict; PG suites 22/22.

`get_design_context`, one frame (ms): 200k PG cached 156 → RAM warm 1.5; right after a
write 207 → 2.0. Real projects (6–7k nodes): cached 2.9–4.7 vs RAM warm 3.3–5.7 (RAM
rebuilds slice indexes, 0.2–0.6 ms), right after a write 5.5–7.7 vs 3.5–5.7. The rest is
the reader itself (3–5 ms): I5's reader rewrite is where real projects gain.

Open for I3: tag incarnation when the mirror loads (avoid the first-read reload);
failing reads run twice (fallback reruns them); cache slice indexes or read persistent
indexes (I5); `get_edit_context`/viewports through RAM; whether the per-read DB probe
stays once commits go through RAM (needed only with several processes on one PG).

## I3 — writes through RAM + per-property LWW (done)

Branch `feat/node-memory-writes` from `feat/node-memory-reads` (same worktree). Two
independent parts; the semantics part applies on both paths so results never depend on
the flag.

**A. Semantics (both paths, always on).**
1. `apply_batch` / `apply_batch_file`: `expectedRevision` becomes optional and means "the
   revision my edit is based on". Without `strict:true` it never rejects for staleness:
   the scoped/page guard and the global revision check stop rejecting; the batch is
   applied in commit order (last writer wins per top-level props key; `(parentId,
   sortKey)` is one property; `merge:true` keeps deep merge). With `strict:true` the old
   checks run unchanged (`CONFLICT` as today). Keep one field; do not add `baseRevision`.
2. The result reports `concurrent` when `expectedRevision` < current revision: up to 20
   `{nodeId, key, revision}` pairs that commits after `expectedRevision` wrote and this
   batch overwrote (key = props key, `placement`, `created`, `deleted`; tokens/styles/
   notes by id), plus `concurrentTotal`. Compute it from commit history
   (`commits_after`/`ops_json`) so both paths agree; the lib's stamps may answer it on
   the RAM path only if results are identical. History older than retention →
   `concurrentUnknown: true`.
3. Only invariant violations reject (cycle, missing/deleted parent, component in use,
   schema/props/validation errors, virtual-child writes, limits). Error messages must
   name the node, field and the current value/parent the agent needs to fix the batch
   without a re-read.
4. Idempotency, history, `get_changes`, SSE cursors unchanged. `import_asset` and other
   non-batch writes keep their current `expectedRevision` rule.
5. MCP schemas, `instructions.rs`, skills (`author-frame.md`, `ocd.md` within its byte
   budget, props/read docs), `docs/SERVER.md`, `docs/PAGE-CONCURRENCY.md` updated; the
   discovery/tools budget tests must pass.

**B. Commit through RAM (flag `FAGENT_NODE_MEMORY_WRITES`, off by default; implies
reads and mirror).**
1. Per project: one ordered writer. Under it: idempotency check → stage the batch in the
   lib (`begin`) → validation on the staged state with today's validator (build its
   input from RAM, no `entities_for_write`; incremental validation is I4) → persist the
   lib's delta rows in one DB transaction (synchronous, before ACK) → commit the staged
   batch, publish, ACK. DB failure ⇒ RAM rolled back exactly; readers never observe
   staged state. Never hold a `std::sync` guard across `.await`; choose and justify
   (async writer mutex + sync staging, `spawn_blocking`, or a Send-able guard).
2. When the mirror or a write loads a project, tag its incarnation so the first read
   does not reload (I2 open question 1). Keep the per-read DB probe (several processes
   may share one PG).
3. Remove the shadow's double work on this path (the commit already came from RAM); the
   shadow compare stays available in `strict` for tests.

**Decided defaults:** placement stamps only for explicitly placed nodes (renumbered
siblings are a side effect, never `concurrent`); dense renumbering stays (user,
2026-11-16).

**AC:** differential — the same random op sequences (including concurrent writers
interleaved, stale `expectedRevision`, strict on/off) give identical results
(`BatchResult` incl. `concurrent`, errors) and identical DB rows with
`FAGENT_NODE_MEMORY_WRITES` off and on, on SQLite and PG, plus the real-data restore
check (temp cluster, never 2703); storage + server suites pass (server: the known 36 env
failures, none new); PG suites via `scripts/task19-check.sh`; commit latency median (30
commits, PG, scoped) on the 200k fixture and a real project, flag off vs on, with the
same breakdown as Task25 (load, validate, persist, cache); fmt/clippy clean for touched
code; files ≤ 250 lines.

### I3 result

Commits `7b756c4`..`f975d2d` on `feat/node-memory-writes`.

- **Contract (both paths):** `apply_batch` takes `expectedRevision?` and `strict?`. A stale
  base never rejects; commits apply in writer order, last writer wins per top-level props
  key (`merge:true` still deep-merges) and per placement. A success on a stale base adds
  `concurrent` (≤20 `{nodeId,key,revision}`, latest revision per pair), `concurrentTotal`,
  or `concurrentUnknown` (history gap, size cap or >1,000 commits), computed from the
  commits' `ops_json`. `strict:true` requires `expectedRevision` and keeps the old exact or
  `pageIds` guard; a base ahead of the project conflicts in both modes. `pageIds` without
  strict is recorded but guards nothing. Idempotent replay comes first and returns the
  stored result. `import_asset` keeps its exact-revision rule. Structural errors
  (`scene-input/src/validate/structural.rs`) name node, field and current value.
  `bundle_history::replay_request` accepts envelopes without/with older `expectedRevision`.
- **Memory path (`FAGENT_NODE_MEMORY_WRITES` off|on|strict):** SQLite under the
  per-project writer mutex; PostgreSQL in one pool job under the project row `FOR UPDATE`
  lock. Base = `to_snapshot()` of memory (tagged incarnation, at DB revision) → today's
  `validated_candidate_indexed` → lib `begin` → persist touched rows (unchanged rows
  skipped) → DB commit → memory commit. DB failure drops the staged batch and evicts
  (`write_aborts`); lib rejecting a validator-accepted batch evicts and falls back
  (never observed). Mirror loads tag the incarnation (first read hits); SQLite re-tags
  after each commit. Shadow `observe` is skipped on memory commits.
- **Differential:** rows and responses identical off/on — SQLite 3 seeds × 300 steps,
  PostgreSQL 2 × 200, racing writers 4 × 12 replayed in order, real backup
  restored twice on a temp cluster (7 projects, 793 commits,
  555 with `concurrent`, 0 mismatches, 0 fallbacks).
- **Commit latency, PG, 30 scoped commits, median ms (off → on):** 200k total 636 → 468
  (load 200 → 29, `persist.delta` 34 → 3, validate 202 → 210, `from_snapshot` 63 → 57,
  `cache.fill` 45 → 47); real project 7,025 nodes total 28.5 → 22.5. Validation now
  dominates.

Verified by the main session: storage 213 passed/40 ignored with flags off and with WRITES/SHADOW/READS strict; node-memory 43/1 ignored (release); server lib 158/4 ignored off and strict; touched files rustfmt-clean (render crate fmt drift predates this slice); temp cluster port free.

Open for I4: feed the validator the staged delta plus `instances_of`/`master_of` instead
of a full `Candidate` from `to_snapshot()`; derive `BatchScope`/footprint/`concurrent`
from the lib delta (stamps only if proven identical to history); `cache.fill` from
memory or dropped for HOT projects (I5); SQLite unchanged-row filter scans the base table
(lib pre-image accessor); PG memory path compares against re-serialized rows, so a
legacy non-canonical stored row is not rewritten (none seen in real data).

## I3b — persistent read indexes updated by delta (implemented; deep validation deferred)

Why: I2 serves a RAM read by copying a slice (`ProjectState::slice`) and rebuilding
`RevisionIndexes::new` on it (0.2–0.6 ms) plus the lazy `ScopeIndex` per read; the cached
path builds those once per revision. Each commit already yields an exact `Delta`.

Scope:
1. Per HOT project keep the reader-format indexes (topology, page index, inverse
   references, scope index, dependency index) built once at load.
2. Apply each commit's `Delta`: plain props keys → no index work; reference keys
   (`componentId`, token/style refs, link targets) → update that node's reference rows;
   create/delete/reparent/reorder → update the affected topology/page/scope entries.
3. Publish per revision with structural sharing (persistent maps such as `im`/`rpds`, or
   copy-on-write of touched parts) so readers holding revision N are never mutated; no
   std guard across `.await`; a commit never waits for readers.
4. Readers run on the whole-project view plus persistent indexes: drop the slice copy.
5. Result caches (snapshot bytes, design context) still invalidate the affected region,
   including instances of an edited master on other pages.

AC: differential 0 differences (fixture, 5k, real backup restore on a temp cluster) with
interleaved writes, flags off/on; storage and server suites as in I3; read latency on the
real projects warm and right after a write, median 30, against the cached path; index RAM
per node; follow the module policy above (no hard file-size cap).

### I3b implementation result — 2026-11-16

Implemented immutable whole-project reader views, delta-maintained shared index shards,
and publication after durable commit. HOT scoped readers no longer copy a slice or build
revision/scope indexes per read. Old readers keep their revision without holding the
writer-state lock. Existing flag-off/custom/historical paths and freshness probes remain.

The user explicitly requested code focus and deferred deep tests and all benchmarks until
the implementation work is complete. Cargo check, touched-library clippy/fmt and small
publication/sharing/authority/budget/SQLite/committed-fixture smoke checks passed. This is
**not full I3b AC verification**: generated 5k and real-backup/PG differential, full suites,
30-sample latency and index RAM measurements remain deferred. Details and rerun commands:
`docs/TASK28-I3B-REPORT.md`. I4/I5 are not started by this slice.

## I4 — incremental validation from memory deltas (authorized 2026-11-16)

Continue directly in the separate worktree `FAgent-node-memory`, branch
`feat/node-memory-writes`. Keep I3b, the relaxed module policy above, and existing
uncommitted policy documentation. One new worker owns this slice; no I5 cleanup.

1. On the current HOT memory-write path, stage operations first and validate the
   staged state from its exact `Delta` and revision-bound indexes. Common node edits
   must not build a whole-project `Snapshot`/`Candidate`, payload-position map or
   full instance expansion for validation. Existing full validation remains the
   reference and conservative fallback for operation classes not safely localized.
2. Derive the affected closure using old and new ancestry, master ownership and
   instance usage, including nested instances and consumers on other pages. Expand
   and resolve only affected instances and required dependencies, across every
   relevant theme; unchanged validated nodes/resources remain trusted only at the
   existing builtin identity/incarnation/revision boundary.
3. Preserve validation invariants, error contracts/order, batch atomicity, rollback,
   durable-before-ACK publication, LWW, strict/page guards, history/idempotency/SSE
   and concurrent reporting. Use committed/pre-image rows or indexed lookups for
   footprint/history needs; retain the history oracle for concurrent reporting.
   Explicitly classify full fallbacks, including global/resource/theme operations
   where needed, rather than treating an incomplete closure as a full project.
4. Wire both SQLite and PostgreSQL. Keep flag-off/custom/import/historical paths
   and I3b reader publication intact. Full snapshot cache fill and removal of old
   cache/guard layers remain I5; do not hide full validation work inside another
   phase or claim the 2–5 ms target without the later benchmark.

Current verification scope (user decision): compile, format, touched-library lint
and a few small meaningful smoke cases for accept/reject parity, cross-page master
dependencies and rollback. No full suites, generated 5k/200k runs, PG restore/matrix,
latency or RAM benchmarks now. Include focused test sources and rerun commands for
the later full verification phase; label unverified ACs explicitly.

Output: local commits on the same branch; API/module summary and fallback matrix;
`docs/TASK28-I4-REPORT.md` with actual checks, remaining whole-project costs and
deferred tests/measurements. Above 500 lines consider splitting; above 1000 explain
the decision. Never split files or introduce abstractions solely for line counts.

### I4 implementation result — 2026-11-16

HOT SQLite/PostgreSQL commits stage first and validate a bounded old/new affected
closure with persistent revision indexes, transitive master users and required
dependencies. Exact-view admission runs full validation once; uncertified legacy
loads cannot grant partial trust. Prefix replay preserves per-operation errors;
delta-maintained expansion totals keep the full-project node cap. Persistence and
history footprints use indexed preimages, with durable-before-publish isolation.
Feature strict mode retains a full reference oracle; conservative global/resource/
theme/note/navigation/root-order and stale strict-page paths remain explicit fallbacks.

Only targeted compile, fmt, touched-library lint and small scene/SQLite/publication
smokes were run. Full differential, disposable PG verification and timing/RAM remain
open. No 2–5 ms or full parity claim. PG snapshot cache fill remains full work for I5.
See `docs/TASK28-I4-REPORT.md` for APIs, fallback classes and exact rerun commands.

## I5 — HOT reader and cache cleanup (authorized 2026-11-16)

The user requested pushing the completed work, then continuing I5. I4 checkpoint
`42a4d8a` was pushed and verified at `origin/feat/node-memory-writes`. Continue
directly on that branch in the separate worktree `FAgent-node-memory`, with one
new worker. Preserve I3b/I4, flags, wire/storage schemas and current safety boundaries.

1. HOT reads use the immutable persistent view/indexes directly. Replace repeated
   authored `TreeIndex`/payload-map construction and per-read adjacency sorting in
   context/subtree/outline/find, and route metadata/resource/list reads through the
   same capability where applicable. Retain exact output bytes, sibling/source
   ordering, virtual IDs, pagination/budgets, exact find totals, component definitions,
   themes/resources/notes/navigation and errors. Computed/render preparation must
   use one consistent revision and resource set. Work required to produce requested
   output or search all requested nodes remains real work, not a bounded-search claim.
2. For current HOT projects, remove duplicate retained full snapshot/Moka/legacy
   revision-index work from normal reads and successful commits. In particular, PG
   commits must not reconstruct the full project merely to perform `cache.fill`.
   Explicit full snapshot/HTTP/export/bundle APIs may materialize a full snapshot on
   demand; keep their complete wire contract. Cold/stale/off/custom/historical paths
   retain conservative behavior. Do not move duplicate caches into another layer or
   invalidate unrelated scene/render caches as a substitute for fixing ownership.
3. Replace redundant HOT scoped guard/index plumbing with persistent-index adapters;
   preserve requested strict/page guard semantics, history coverage, LWW/concurrent,
   idempotency and durable-before-ACK publication. Legacy implementations remain
   where fallbacks require them. Keep DB freshness/incarnation/schema/readiness probes
   while multiple processes can share PG, plus TTL/LRU accounting and old-reader
   isolation. Reconcile external writes/imports/restore and cache eviction correctly.
4. Implement useful typed props access in the HOT reader/engine path while preserving
   every authored JSON field, merge semantics and round trips. The proposal's N7
   (`docs/NODE-SERVICE-PROPOSAL.md` in the original checkout) describes optional full
   typed packing/string interning; do not claim its heap-reduction target from a thin
   wrapper or unmeasured accessor change. Explain the chosen boundary and any remaining
   JSON representation in the design/report before making a large schema refactor.

Current checks remain code-focused: compile, format, touched-library lint and a few
small meaningful smoke checks of reader parity/cursors, cache bypass/freshness,
strict guards and rollback/retained revisions. No full suites, generated 5k/200k,
disposable PG/restore matrix, latency/RAM benchmark or native runtime changes now.
The read <1 ms/RAM goals remain unverified until the later measurement phase.

Output: cohesive local commits on `feat/node-memory-writes`, updated task status and
`docs/TASK28-I5-REPORT.md` with API/ownership/cache/guard matrix, typed-props boundary,
actual commands/results, remaining global costs and deferred verification commands.
Root reviews and runs bounded checks before completion. No deployment or new feature
scope. Apply the >500 consider-split / >1000 explain policy, with no artificial cap.

### I5 implementation result — 2026-11-16

Accepted code uses persistent authored traversal/order for outline/subtree/context/find,
project-wide MCP reads and resource lists; common borrowed typed props preserve raw JSON.
HOT projects no longer retain duplicate legacy snapshot/revision-index caches, and PG
HOT commits skip cache-fill reconstruction. Identity probes, I4 publication and existing
strict/page guard contracts remain. Targeted compile/lint/fmt and small fixture oracles
passed; no timing/RAM/PG/full differential claim.

After the partial checkpoint, the user explicitly approved the two exact guard/reader
artifacts ("handle the last 2 patches"). A new implementation subsession applied both through
normal review. Supported HOT stale strict-page writes now use old/new persistent
dependency closures with contiguous, versioned, sealed history coverage; unsupported
classes keep the legacy fallback. MCP edit-context uses the shared exact wire formatter.
Domain prerender, MCP/HTTP scenes and HTTP fonts now consume the reader capability;
explicit full snapshot/export/bundle paths retain their complete contract.
Small guard/history/summary byte parity, actual SQLite stale dispatch/conflict/rollback
and retained-reader smoke, scene/key/revision/resources/note-intent, consumer metadata
and full-snapshot checks passed. I5 code is complete within the authorized scope;
deep differential, PostgreSQL runtime/restore, timing and RAM remain unverified.
See `docs/TASK28-I5-REPORT.md`. Do not auto-enter the deferred validation phase.
