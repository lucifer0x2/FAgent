# Task 41 report — self-contained node-memory engine

Spec: [tasks/41-node-memory-engine.md](../tasks/41-node-memory-engine.md). One section per phase.

## Phase 1 — Boundary (2026-11-18)

Branch `feat/task-41-node-memory-engine`, commits `fe03415..6cc6c67` (10 commits). Memory is always
on: every latest-revision read and every batch on SQLite and PostgreSQL goes through
`fagent_node_memory::NodeMemory`. The `FAGENT_NODE_MEMORY_*` flags, the shadow mirror and the
`off`/`on`/`strict` modes are gone. Diff over the phase: 144 files, +4,444 / −4,491; in
`crates/storage/src` alone +1,875 / −3,335.

### Engine API (`crates/node-memory`, deps unchanged: scene-input, serde, serde_json)

- `NodeMemory::new(MemoryConfig, Arc<dyn Clock>)`; `MemoryConfig { ttl: 1 h, budget_bytes: 2 GiB,
  expire_every: 60 s, limits }`.
- `load(Snapshot) -> Result<HotProject>` (structure only in phase 1; a racing load returns
  `PROJECT_EXISTS`), `project(id)` (refreshes access; a diverged project is evicted and `None`),
  `resident(id) -> Option<LoadId>`, `evict`, `expire`, `maybe_expire` (rate-limited by `Clock` and a
  CAS stamp; never spawns), `to_snapshot`, `stats() -> MemoryStats` (per-instance counters: loads,
  load failures/µs, evictions, expired, over budget, commits, rollbacks, local, full, rejected,
  divergences), `total_bytes`.
- `HotProject` (handle + monotonic `LoadId`): `view`, `settled_view(revision)`, `diverged`,
  `estimated_bytes`, `begin(expected_revision, ops) -> Result<Begin>` with
  `Begin = Ready(Prepared) | Stale`.
- `Prepared`: `base_revision`, `revision`, `changed_ids`, `rows()` (only rows that differ from the
  base), `footprint()` (`node:`/`page:`/`note:`/`token:`/`style:`/`*` for every op class),
  `before`/`after` views, `path()` (`Local`/`Full`), `commit() -> Delta`; drop rolls back. It holds
  the project write lock until commit or drop.
- Validation inside `begin`: classes the classifier supports stage and run `validate_delta`; a
  `Txn` rejection or an undecided delta runs the reference (`to_snapshot` → `Candidate` → per-op
  `apply` with op index → `prune_links` → `validate_after`), all in memory. Other classes stage
  and run the reference. A reference accept after a `Txn` reject is a divergence:
  `STORAGE_FAILED`, counted, logged with the project id only, and the project is evicted on next
  access.
- `max_nodes` is checked at `finish` on the final state, without an op index, as `Candidate` does.
- An update that changes no value writes no row and bumps no stamp; `props::same` compares by value
  and then by serialized text, so `0.0` → `-0.0` is a change (as the stored text sees it).

### Storage adapter (`crates/storage/src/memory*`)

- `memory.rs` + `memory/{identity,reads,writes,guard,stats}.rs`. The adapter keeps
  `project → Tag { load_id, schema, incarnation, readiness, revision, failed, cache_cleaned }`;
  the engine stores no identity.
- Freshness: under the SQLite writer mutex or the PostgreSQL `FOR UPDATE` row lock, the adapter
  compares the database revision and incarnation with the settled view; a stale copy is evicted and
  reloaded. A writer racing another thread's publish waits for it instead of reloading. Readers
  and previews never replace a resident copy at or past the snapshot they loaded.
- A load error from the engine is remembered per identity and returned as `VALIDATION_FAILED`.
- SQLite commit (`batch/memory.rs`): writer lock → idempotency replay → probe → `for_write` →
  base check → `begin` → history → `guard` → result → persist changed rows → `commit()` → retag →
  cache cleanup claim. Dry run takes the same path and persists nothing.
- PostgreSQL commit (`postgres/batch.rs`): `FOR UPDATE` → replay → base check → identity →
  `for_write` → `begin` → history → `guard` → persist → `tx.commit` → `commit()`. Preview uses a
  repeatable-read snapshot, retries once on `Stale` or when memory is ahead, and on its last try
  stages on a private engine instead of evicting the newer copy.
- A database error after staging drops `Prepared`, counts an abort and evicts.
- Evictions: asset/font import, privacy update, `open_project`, database error after staging.
- Moka (`hot_cache.rs`) is read-through for every project; commits and imports evict the entry
  (no more `fill`). Entries match `(generation, revision)`, so a late put of a pre-commit
  snapshot is never served (test `a_late_fill_of_the_pre_commit_snapshot_is_never_served_at_the_new_revision`).
- New: `Storage::memory_stats()` (`NodeMemoryStats`: engine stats, resident bytes, read
  hits/fallbacks/loads/reloads/errors, write hits/loads/aborts), `memory_read_failed()`,
  `evict_memory`, `memory_snapshot`.

### Removed

- `crates/storage/src/shadow.rs` and `shadow/*` (compare, incremental, mirror, read/write modes,
  reads, rows, stats, writes, tests), `batch/incremental.rs`, `batch/commit.rs`,
  `postgres/shadow_hook.rs` (`shared_with_identity` moved to `postgres/adapter.rs`).
- `ShadowMode`, `ReadsMode`, `WritesMode`, the `NODE_MEMORY_*_ENV` constants, `Store::open_with_*`,
  `PostgresConfig.node_memory_*`, `node_memory_shadow_stats`, the persist row sink,
  `Written::shadow_rows`, `Committed::observe`/`from_memory`, the PostgreSQL `FALLBACK` retry,
  `SnapshotCache::with_owner`/`IndexCache::with_owner` and their owner bypass, `ScopedRead.verify`.
- Suites: `node_memory_shadow{,_bench,_realdata}`, `shadow_realdata/`,
  `node_memory_writes_realdata` (an admission run over a restore returns in phase 2), and the
  flag-forcing test helpers.

### Kept, with consumer

- Test-only until phase 4 (oracles for `memory/guard_tests.rs` and the scoped parity tests):
  `scoped::check`, `BatchScope`, `scoped::notes::check`, `batch/validation.rs`,
  `DependencyIndex::from_candidate`, `concurrent::fill`, `RevisionIndexes::{candidate, publish}`.
- Production: `scoped::{request_value, envelope_with_scopes, coverage, PageIndex::validate_page_ids,
  edit_context}` and the `IndexCache` read side (history envelopes, page ids, edit context,
  past-revision reads); `Stored::delta`, `Project::persist` and `Candidate` (asset import, bundles,
  oracles); Moka for full-snapshot reads (`/snapshot`, export, `get_changes`), decided in phase 4.
- `classify::supports`, `validate_delta` and the engine's reference path: removed in phase 3.

### Verification

- Differential (`crates/node-memory/tests/differential.rs::begin_matches_candidate`): seeds 1–12,
  300 batches each, 3,600 batches; 1,069 accepted (857 committed, 212 dropped to exercise
  rollback; 214 local, 855 full), 2,531 rejected across 25 code/field kinds. **0 differences** in
  accept/reject, rows, changed ids, revisions, committed state, and per-op code/field/op
  index/message; 0 end-of-batch order differences; 0 divergences. The generator now covers pages,
  root frames and moves, tokens with themes and aliases, styles and users, `theme`, links and
  `flowStart`. Canary: dropping the row filter produced 578 differences.
- Guard parity (`memory/guard_tests.rs`): 42 cases over every op class × 9 page sets × 6 history
  shapes (current, old unrelated, old overlapping, legacy without scope, gap, base ahead), more than
  1,500 comparisons with `scoped::check`/`BatchScope`: identical changed ids, footprint, guard errors
  and `BatchResult`.
- `node_memory_writes`: a warm store against a store evicted before every request; after every
  commit the memory snapshot equals the database snapshot; at the current base every result or
  error equals `Candidate` apply + `prune_links` + `validate()`. PostgreSQL run: 243 committed, 81
  rejected, 35 conflicts, 174 checked against `Candidate`, 0 divergences.
- New `postgres_memory_freshness`: two backends on one namespace; after B commits, A never reads
  or validates against its stale copy.
- Suites: scene-input + node-memory + storage on SQLite 351 passed, 0 failed; server 358 passed with
  the 41 known environment failures (render worker `WORKER_UNAVAILABLE`, `startup_env` stderr),
  same names as before the phase; PostgreSQL default suite 21 passed; targeted PostgreSQL suites
  (memory writes/reads, revision indexes, scoped reads, cache, dry run, write-through, freshness)
  16 passed; server `postgres_backend` 2 passed.
- `postgres_dry_run` (2 tests) and one `postgres_write_through` test already failed on `main`: they
  sent non-strict requests at an old base and expected `REVISION_CONFLICT`, which only strict
  requests get. The tests were made strict; no behavior changed. `postgres_write_through` was
  rewritten for read-through caching.
- fmt and `clippy --all-targets -D warnings` are clean for node-memory, scene-input, storage and
  server (pre-existing lints fixed in `6cc6c67`). `rg FAGENT_NODE_MEMORY_` finds only
  `docs/agents/LOG.md`, closed Task 28 reports, `tasks/28` and `tasks/41`.

### Benchmark (PostgreSQL, release, generated fixture of 178,604 nodes, small props edit, median of 30)

| Path | Total | Main phases |
| --- | ---: | --- |
| Phase 1 (always on) | **36.5 ms** | `memory.begin` 30.4, persist 4.8, history envelope 0.02 |
| `main`, opt-in memory path | 55.9 ms | `memory.validate.delta` 50.1 |
| `main`, default database path | 988 ms | load, `Candidate`, full validate, `cache.fill` |

The three rows were measured back to back on an idle machine. Under heavy machine load (load
average ~47 on 10 cores) the same code measured 70–106 ms: threads land on efficiency cores, so
compare runs only back to back at low load and read min/p10 as well as the median. The bench does
not print p95. An earlier idle run of `main` gave 38.1 ms (memory) and 794.5 ms (database).

The cold first commit on a 200k project costs about 3.8 s (load 0.35 s, engine load 0.87 s,
admission certificate 2.4 s) and runs under the row lock. `ea3286c` fixed a quadratic build of the
read-view index sets that made this 13 s at 45k nodes and timed out at 200k.

### Behavior changes

- Commits leave the project resident at the new revision; the next read is served from memory.
- The first PostgreSQL commit on a cold project loads and admits it while holding `FOR UPDATE`.
- PostgreSQL preview/dry run waits for a staged in-memory batch and can make a cold project HOT.
- Ops the classifier does not support (pages, root ops, tokens, styles, notes, props
  `theme`/`links`/`flowStart`/`animations`) still validate the whole project, now in memory with
  no database load or cache fill. Phase 3 replaces this.
- When a batch has several end-of-batch errors, the first one reported may differ; per-op errors
  keep their op index. The node cap error carries no op index.
- Persist now writes `0.0` ↔ `-0.0` changes on SQLite (previously skipped by value equality).
- SQLite outside Unix has no file stamp, so every access evicts and reloads: correct but slow
  (known limit).
- Internal API breaks: modes, `node_memory_shadow_stats`, `Store::open_with_*`,
  `PostgresConfig` memory fields, `ScopedRead.verify`; counters are per instance.

### Open

- No admission on load yet: stored data is trusted as before (phase 2: serve, count and log).
- Validation of a small props edit is ~30 ms against the < 5 ms target (phase 3).
- Spike 1b (single persistent state; `imbl` approved by the user on 2026-11-18) decides the engine
  structure before phase 2.
- Files: none over 300 lines among the new modules; largest touched source file
  `crates/storage/src/postgres/adapter.rs` (256 lines).

## Spike 1b — One persistent state (2026-11-18)

Branch `spike/task-41-1b` (from `feat/task-41-node-memory-engine`). Measurement only: the engine is unchanged.
Prototypes live in `crates/node-memory/examples/bench/spike/`; `imbl` 7.0.2 is a dev-dependency of the bench only.
Rerun, one option per process so memory rows stay separate:

```bash
cargo run --release -p fagent-node-memory --example bench -- --runs 101 --load-runs 7 --spike a   # also ord, hash, radix
```

### What was compared

- **(a) arena** — today's engine through `Registry`/`ProjectHandle`: slot arena + undo log (`ProjectState`) and the
  published `ReadView` (256-shard persistent maps keyed by `String`). `begin` stages under the write lock, `commit`
  advances and publishes the view, drop rolls back.
- **(b) imbl** — one persistent state: `nodes: Map<u32, Rec{Arc<Node>, parent, page, root, master}>`, sorted children
  `Map<u32, Arc<Vec<u32>>>`, pages, instance users and incoming links (`Map<u32, Arc<BTreeSet<u32>>>`), ids interned to
  dense `u32`. `begin` = O(1) clone, ops write on the clone, reject = drop, commit = publish that clone (pointer swap).
  Delta: `OrdMap::diff` skips shared subtrees; `imbl::HashMap` has no diff, so it uses the write set. Measured with
  both `OrdMap` and `HashMap`.
- **(c) radix** — the same state over a hand-written persistent radix trie (64-way, depth grown on demand: 3 levels at
  200k keys, ~200 lines, no dependency); pointer diff skips shared nodes.
- Same workload for all: ops of the stored `Operation` type with the arena's placement rules (renumber to positions);
  rows counted per commit match across options (1, 14,005, 1, 328, 1). The read is the structural part of
  `get_design_context` on one frame (`ReadView::expand_view` steps: subtree + ancestors + master closure, ordered);
  instance expansion and resolution run after it on the node list and do not depend on the store.

### Results (200k fixture: 178,415 authored nodes; release; 101 runs, 7 loads; load average 1.7–2.4; two rounds in
alternating order, round 1 shown, round 2 within ±10%)

| Measure | (a) arena + ReadView | (b) imbl OrdMap | (b) imbl HashMap | (c) radix trie |
| --- | ---: | ---: | ---: | ---: |
| Footprint after load, per authored node | 3,280 B | 1,633 B (−50%) | 1,722 B (−48%) | **1,563 B (−52%)** |
| Load + index build, median / p95 | 372 / 398 ms | 38.8 / 39.5 ms | 40.0 / 40.9 ms | **27.4 / 28.1 ms** |
| Get node by id | 95 ns | 67 ns | 38 ns | **20 ns** |
| Read: frame closure `root_10` (291 nodes), median / p95 | 91 / 98 µs | 60 / 65 µs | 29 / 32 µs | **22 / 31 µs** |
| Read: frame closure `frame_wide` (14,002 nodes) | 5.70 / 5.85 ms | 2.42 / 2.49 ms | 1.65 / 2.02 ms | **1.14 / 1.17 ms** |
| Commit: 1 props update (begin + apply + delta + publish) | 19.9 / 21.0 µs | 1.29 / 1.46 µs | 1.04 / 1.25 µs | 1.08 / 1.67 µs |
| — apply (begin + op) | 0.50 µs | 0.54 µs | 0.71 µs | 0.50 µs |
| — publish per commit | 19.5 µs (view advance) | 0.25 µs + 0.50 µs diff | 0.33 µs | 0.29 µs + 0.33 µs diff |
| Reject: 1 props update (dropped) | 0.58 µs | 1.25 µs | 1.04 µs | 1.08 µs |
| Commit: create node (append) | 83 / 95 µs | 4.5 / 5.9 µs | 2.9 / 6.5 µs | **1.4 / 1.9 µs** |
| Commit: create at index 0 of wide frame (14k renumbered, 5 runs) | **8,975 ms** | 5.95 ms | 10.6 ms | **3.31 ms** |
| Commit: reparent subtree across root frames | 4.15 ms | 48.7 µs | 73.9 µs | **5.4 µs** |
| Commit: delete root frame subtree (328 nodes) | 210 / 353 ms | 198 / 340 µs | 371 / 605 µs | **104 / 193 µs** |
| Commit: 100 props updates in one batch | 48 µs | 49 µs | 50 µs | 37 µs |

Notes:

- The ~1.3 KB/node and 584/542 ns baselines from phase 1 were `ProjectState` alone (no `ReadView`). The engine holds
  both, so (a) is 3,280 B/node, and a committed small edit costs ~20 µs, almost all in the `ReadView` advance.
- **Phase-1 finding, independent of the decision:** publishing to `ReadView` is superlinear in the changed rows.
  `ViewIndexes` updates one sibling at a time and each update clones and re-weighs the parent's whole
  `Arc<BTreeSet<(i64, String)>>` (`scoped/view_indexes.rs::sibling`, `persistent::Map::insert`), so renumbering 14k
  siblings takes ~9 s and deleting a 328-node root frame ~210 ms, all under the project write lock. Any reorder or
  insert at the front of a wide frame hits this today.
- The prototypes leave out what is not on the measured paths: per-prop write stamps, resources and notes, the
  `ReadView` reference/dependency/frame indexes and structural checks at load (cycle, depth, limits), so their load
  time is a lower bound. Even if the missing indexes cost as much again as the arena alone, RAM would stay below
  (a); the decision margin on RAM is large (52% against a 30% bar).
- Renumbering in (b)/(c) copies each changed node (props included), where the arena edits in place; still 3–6 ms for
  14k siblings because no sibling set is copied per member.
- `imbl::HashMap` has no structural diff; its delta comes from the write set. Its read and lookup are faster than
  `OrdMap`, its publish (dropping replaced nodes) slower.

### Decision

All three criteria pass for (b) and (c): RAM/node −48% to −52% (≥ 30%), a small in-memory commit 1.0–1.3 µs
(≤ 50 µs), reads 1.5–4× faster. **Recommendation: (c), the hand-written radix trie.** It is best or tied on every
row, needs no new dependency, and its pointer diff gives the delta for free; `imbl` stays out of the runtime
(`OrdMap` is a close second if a maintained library is preferred).

What adopting (c) means for phase 2 (needs the user's go-ahead):

- One `Arc<State>` per project is both the writer base and the published view; `Txn` writes on a clone, so the undo
  log (`store/undo.rs`), the second node copy in `ReadView` and `state/publication.rs` go away. New phase-3 indexes
  are more persistent maps in the state, with no undo entries.
- Readers look ids up through the interner, so it must be safe to read while one writer appends (append-only
  `RwLock` or a persistent string → key map inside the state). Interned keys are never reused; deleted ids stay
  interned until the project is reloaded (eviction/reload already bounds this).
- `ReadView`'s scoped readers and the validators read `String`-keyed maps today; they need an adapter or the phase-2
  lookup trait over the new state.
- Keep the arena only as a test oracle during the switch, then delete it.

## Phase 2 — Persistent state, validator trait, per-op checks, admission (2026-11-18)

Branch `feat/task-41-phase-2` (from `spike/task-41-1b`), commits `10a3727..c649057` (13 commits) plus this report.
Diff over `crates/node-memory/src` and `crates/scene-input/src`: 69 files, +2,600 / −1,692. Largest touched source
file: `crates/node-memory/src/store/trie.rs` (327 lines); none over 1000.

### Decision (c) of 2026-11-18: one persistent state over a radix trie

The user chose option (c) of spike 1b. The slot arena, its undo log (`store/undo.rs`) and the separately published
`ReadView` (`state/publication.rs`, `scoped/view_delta.rs`, `scoped/view_indexes.rs`) are deleted. Reason: publishing
to `ReadView` was superlinear in the changed rows (14k-sibling renumber ~9 s, 328-node root-frame delete ~210 ms, both
under the project write lock). Regression tests: `crates/node-memory/tests/publish_cost.rs`.

- `ProjectState` is one persistent value: nodes and indexes live in a hand-written 64-way radix trie
  (`store/trie.rs`, no dependency) keyed by interned ids. Clone is O(1); the delta is a pointer diff that skips
  shared subtrees.
- `ProjectState::stage(&self)` writes on a copy-on-write clone. Reject = drop the clone (no undo log). Commit publishes
  the same `Arc` that served as the validation preview; `ReadView` wraps an `Arc<dyn ViewSource>`, which
  `ProjectState` implements (snapshots use `SnapshotSource`). `imbl` stays a bench-only dev-dependency.
- Ids go through one shared append-only interner (`store/intern.rs`), safe to read while one writer appends. A
  rejected batch gives its names back: the interner records its length when a batch opens and truncates back on
  close only when the batch is discarded, no other batch was open or opened since, and no live version (escaped
  preview, `Prepared::after`, state clone) can hold its keys. Otherwise the names stay and their bytes are added to
  the handle's estimate. Test: 200 rejected batches with 10 KB distinct names grew the interner by 8.07 MB before
  the fix, < 4 KB after.

### Part B: validator lookup trait and per-op checks in `Txn`

- `scene_input::validate::Lookup` (`validate/lookup.rs`) is an object-safe, read-only trait. `Candidate` and the
  engine store (`node-memory/src/store/lookup.rs`) implement it; the public `validate_tokens(&dyn Lookup)`,
  `token_value`, `check_node(lookup, node)` and `check_style(lookup, style)` run on either. A unit test compares
  every `Lookup` answer of the store with `Candidate` on a fixture with sort-key ties, notes and themed tokens.
- `Txn` runs the property checks in `Candidate::apply` order, on the working state through `&dyn Lookup`:

| Op | Order in `Txn` |
| --- | --- |
| create | id/duplicate/note checks, parent and depth pre-checks, insert, `check_node`, place (index check last) |
| update | `set_props`, `check_node` |
| reparent | parent/cycle/depth pre-checks, unlink, set parent, `check_node`, place, then the descendant-height check |
| upsert token | id check, put; `validate_tokens` runs once in `finish` when the batch upserted a token |
| upsert style | `check_style` (includes the id check), put |

- `Rejection { error, exact }`. Exact errors return straight from `HotProject::begin` with the same code, field and
  op index as `Candidate`. Routed (inexact) errors run the reference through `HotProject::rejected`:
  - the reparent descendant-height check (`Candidate` leaves descendant depth to full validation, no op index);
  - `finish` errors: the node cap and `COMPONENT_IN_USE` of unmarked masters;
  - the once-per-batch token validation failure, and every error after a token upsert in the batch: an earlier
    upsert may have left a token set that `Candidate` already rejected at that earlier op.
  The empty-batch error is exact (engine input contract).
- Token validation once per batch: a batch whose token set is invalid mid-batch and repaired later is accepted by
  `Txn` alone, but the engine still rejects it exactly, because token batches always run the reference.
- Counters: `MemoryStats.rejected_direct` and `rejected_routed` (both also counted in `rejected`; a routed
  rejection the reference accepts is a divergence). Storage `NodeMemoryStats` embeds `MemoryStats` as `memory`.
- Test fixtures that relied on unchecked props were fixed (unknown keys, link key `trigger` → `on`, text without a
  font, tokens/styles created before use).

UpsertToken cost, 200 upserts in one batch (release, load average ~3.0, back to back):

| Tokens | `Txn` before / after | `begin` before / after | `Candidate::apply` |
| ---: | ---: | ---: | ---: |
| 500 | 74.5 / 1.4 ms | 94.2 / 74.9 ms | 46–49 ms |
| 2,000 | 212.0 / 1.9 ms | 430.2 / 212.8 ms | 210–213 ms |
| 4,000 | 447.5 / 3.6 ms | 899.2 / 465.3 ms | 444–469 ms |
| 2,000, one upsert | 1.7 / 1.8 ms | 4.4 / 4.5 ms | 1.2–1.3 ms |

`begin` now costs about the reference alone, still O(ops × tokens).

### Admission at load

- `NodeMemory::load` calls `scene_input::validate::memory::admit(&ReadView)` once after the registry load: a full
  `Candidate::validate`. The load never fails on its result; invalid stored data is served. Structural checks (ids,
  parents, cycles, depth, non-object props) still fail a load; the instance-target check moved from load to
  admission.
- Certificate (keeps commits diff-only): a valid revision is certified from `instances::expand`. An invalid revision
  is certified (`valid: false`, inherited by later deltas) only when the project-wide checks pass that
  `validate_after` repeats over every row but `validate_delta` checks only on the closure (or never, for notes):
  notes, navigation, animation triggers and the instance graph. An expansion failure (dangling or non-master
  `componentId`, depth, `INSTANCE_LIMIT`) gives no certificate. A delete on an invalid certified base that cascades
  to notes takes the reference path.
- Counters: `admissions`, `admission_failures`, `admission_micros` (`load_micros` excludes admission).
- A failure logs one JSON line `{"event":"node_memory_admission","project","code","field","node"}`: ids and codes
  only, and `field` cut to its fixed first segment (before the first `.` or `[`), because later segments can be
  user-written keys, state names or override targets. Never message or content.
- Tests (`tests/admission.rs`): a valid base is certified; `width:false` is served, counted, certified, an unrelated
  commit is local and accepted, a touching commit gets the reference error; four corrupted bases (empty note, link
  to a non-root frame, non-ancestor trigger, duplicate `flowStart`) × five batch kinds match the reference; dangling
  token alias and broken `componentId` cases below.

### Measurements

Engine through `Registry`/`ProjectHandle`, 200k fixture (178,415 authored nodes), release, 101 runs, 7 loads, two
rounds at load average 2.0–2.9 (round 1 shown; a range where the rounds differ by more than 10%):
`cargo run --release -p fagent-node-memory --example bench -- --runs 101 --load-runs 7 --spike a`.

| Measure | Spike 1b (a) arena + ReadView | Spike 1b (c) radix prototype | Phase 2 |
| --- | ---: | ---: | ---: |
| Footprint after load, per authored node | 3,280 B | 1,563 B | **1,594 B** (−51%) |
| Load + index build, median / p95 | 372 / 398 ms | 27.4 / 28.1 ms | **54.9 / 55.8 ms** |
| Get node by id | 95 ns | 20 ns | 24 ns |
| Read: frame closure `root_10` (291 nodes) | 91 µs | 22 µs | 74–90 µs |
| Read: frame closure `frame_wide` (14,002 nodes) | 5.70 ms | 1.14 ms | 4.16–5.10 ms |
| Commit: 1 props update | 19.9 µs | 1.08 µs | 3.71 µs |
| Reject: 1 props update | 0.58 µs | 1.08 µs | 3.58 µs |
| Commit: create node (append) | 83 µs | 1.4 µs | 3.96 µs |
| Commit: create at index 0 of wide frame (14k renumbered) | 8,975 ms | 3.31 ms | **5.01 ms** |
| Commit: reparent subtree across root frames | 4.15 ms | 5.4 µs | 7.38 µs |
| Commit: delete root frame subtree (328 nodes) | 210 ms | 104 µs | **536 µs** |
| Commit: 100 props updates in one batch | 48 µs | 37 µs | 193 µs |

- Apply now includes the per-op property checks (`check_node`, ~3 µs per op against ~0.5 µs), hence the slower
  1-op commit/reject and the batch of 100 updates.
- Reads go through `ReadView` over the `ViewSource` adapter (string ids through the interner), so closure reads are
  at arena level, not prototype level. Load includes what the prototype left out (stamps, resources, notes,
  structural checks).

Admission on the 200k fixture (`NodeMemory` + `SystemClock`, 7 loads, load average ~2.6): engine load median 54 ms,
admission median **886 ms** (877–947), load wall ~940 ms; for reference `Candidate::from_snapshot` 55 ms + `validate`
713 ms. The first commit after load is local, **27–29 ms**; phase 1 paid the lazy certificate (~2.4 s) on the first
commit. Warm commits ~26 ms (`HotProject::begin` with `validate_delta`).

PostgreSQL `node_memory_writes_bench` (`WRITES_BENCH_NODES=200000`, 178,604 nodes, small props edit, median of 30,
load average 2.0–2.4, through `scripts/task19-check.sh`):

| Run | Total | `memory.begin` | persist | `validate.instances.expand` | history |
| --- | ---: | ---: | ---: | ---: | ---: |
| Phase 1 (report above) | 36.5 ms | 30.4 | 4.8 | – | 0.02 |
| Phase 2, round 1 | **31.8 ms** | 26.1 | 4.4 | 3.2 | 0.02 |
| Phase 2, round 2 | 32.4 ms | 26.4 | 4.4 | 3.3 | 0.02 |

### Verification

- Differential, state level (`Txn` against `Candidate::apply`, errors compared as full JSON): seeds 1–8 plain and
  1–16 with property ops; 1,184 accepted, 3,605 rejected over 30+ rejection kinds (props, width, color, token and
  style ids, token alias/type/name/themes, style kind/value, `UNKNOWN_THEME`). 0 differences outside the documented
  once-per-batch token case (397 batches, oracle `after_token`).
- Differential, `begin` against `Candidate` apply + validate: plain seeds 1–24 (7,200 batches, 2,078 accepted) and
  property seeds 101–116 (4,800 batches, 895 accepted; per-op rejections 2,975 direct, 786 routed, all routed ones
  at or after a token upsert). **0 differences**, 0 divergences.
- fmt and `clippy --all-targets -D warnings` clean on scene-input, node-memory, storage and server.

| Suite | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| scene-input + node-memory | 143 | 0 | 2 |
| SQLite storage + server | 592 | 41 | 52 |
| PostgreSQL default (`scripts/task19-check.sh`) | 19 | 0 | 0 |
| PostgreSQL targeted (memory writes/reads, revision indexes, scoped reads, dry run, write-through, freshness) | 12 | 0 | 0 |
| Server `postgres_backend` | 2 | 0 | 0 |

The 41 SQLite failures are environmental and have the same names before and after the phase: the worktree has no
renderer build (render worker `WORKER_UNAVAILABLE` and its downstream checks, server startup refusing the Chromium
export backend without `renderer/dist`, `esbuild` missing). The two ignored node-memory tests are the 200k fixture
and the token timing.

### Behavior changes

- A load no longer rejects bad instance targets; admission counts and logs them and the data is served.
- A cold load pays one full validation (admission); the first commit after it no longer pays the certificate.

### Known limitations and open items

Status after phase 3: the first three items and the uncertified-revision item are resolved (pure diff-only
validation, exact per-upsert token checks, no reference path); see "Phase 3".

- A dangling token alias or a broken `componentId` in stored data blocks every commit until repaired, as the
  reference does (the token set is always validated in full; the reference expands every instance). Repair through a
  Full-class batch; the next commit is local. Making this diff-only needs the reference to tolerate stored defects:
  user decision.
- Any stored invalid prop blocks Full-class batches (as before).
- `begin` for token batches still costs O(ops × tokens) on the reference path; incremental token validation (check
  the changed token, its aliasing tokens, theme count and cycles from the changed token) is open.
- Admission adds one full validation to every load, cold reads included (~0.9 s at 200k).
- A revision without certificate (expansion failed) runs the lazy full validation on every local-class commit, as
  before.
- The handle's byte estimate may over-count names of overlapping rejected batches until the next commit. Names
  interned by a batch whose preview escaped stay until eviction (counted).
- The batch of 100 updates is ~4× slower than the arena (per-op checks).
- Phase 3 (delta rules, delete the classifier and the reference path) is not started.

## Phase 3 — Delta rules, no reference path (2026-11-19)

Branch `feat/task-41-phase-2`, commits `f0bc629..68a2382` (5 commits), the bench harness `6257dae` and this report. Diff over
the phase: 74 files, +3,015 / −1,735; over `crates/node-memory/src` and `crates/scene-input/src` 53 files, +1,815 /
−1,349. Largest touched file: `crates/node-memory/tests/support/random_cases.rs` (311 lines); largest touched source
file `crates/node-memory/src/store/mod.rs` (218 lines); none over 1000.

Every batch is now validated by the per-op checks in `Txn` (all exact) plus the end-of-batch delta rules over the
engine state. `Candidate` is off the commit path: it runs once per load (admission) and in the test oracles. The bench
counters confirm it: warm commits record no `validate.*` or `candidate.*` phase, only `memory.txn` and `delta.*`.

### API changes

- `scene_input::validate::Lookup` gains `pages()` (sorted by sort key, id), `referencing(key)` (node ids whose props
  name a `token:`/`style:`/`font:`/`asset:`/`component:`/`link:` key, plus `theme:<name>` for a node's own
  `props.theme`), `aliasers(token)`, `named_themes()`, `declared_themes()` and `note_count()`. `Candidate` answers by
  scans, the store by indexes: the reference index gains the `theme:` key (`reference_keys`), and `Resources` keeps a
  `TokenIndex` (theme refcounts, reverse alias map) in step with `put_token`, so `theme_declared` is O(log n). Token
  envelopes are read leniently (`theme_names`, `alias_targets`).
- `validate::check_token_write(lookup, id, previous)`: the token checks of one upsert with `validate_tokens`'
  precedence (token count; per-token checks of the token, and of its aliasers when its type changed, in id order;
  theme count; alias graph when the token's alias edges changed, with the full graph walked once only on that
  rejection to report its exact message). `validate_tokens`' loop body is the shared `check_token`.
- New `validate::delta` (`affected`, `navigation`, `themes`, `graph`, `expand`, `instances`, `facts`):
  `validate_batch(before, after, &Batch, &Facts) -> Result<Facts>` and `facts(lookup, instances)`. `Facts` holds the
  virtual-node count per authored instance and the expanded total, carried from revision to revision.
- `ProjectState::stage` runs `Txn`, `finish`, then `validate_batch` and stores the new `Facts` in the staged state;
  `ProjectState::apply` alone also runs the end-of-batch rules. `ProjectState::admit()` (called by `NodeMemory::load`)
  runs `Candidate::validate` once for the result and computes `Facts`; a state built without it computes `Facts`
  lazily on its first batch.
- `HotProject::begin` returns every `Txn` and delta rejection directly. `Rejection` lost `exact`/inexact routing.
- Shared checks moved behind `Lookup` so `Candidate::validate` and the delta rules run the same code:
  `validate/instance_checks.rs` (per-node instance checks, properties of expanded nodes), navigation
  (`check_node_links`, `check_flows`, `root_frames`), notes (`check_note`, `too_many_notes`) and
  `animation_triggers::check_triggers`. A `Resolver` overlay may hold a partial expansion and falls back to the lookup
  for ids it lacks.

### Removed

- `crates/node-memory/src/batch/reference.rs`, `HotProject::rejected`, divergence handling (`diverge`,
  `ProjectHandle::diverged`/`mark_diverged`, the `node_memory_divergence` log line), `Path` and `Prepared::path()`.
- `MemoryStats.local`, `full`, `rejected_direct`, `rejected_routed`, `divergences` (`rejected` stays and counts
  per-op and end-of-batch rejections).
- `scene_input::validate::memory` (`classify.rs`, `closure.rs`, `admit.rs`, `validate_delta`, `supports`) and the
  `Certificate` on `ReadView`.
- In `Txn`: the deferred once-per-batch token validation, the reparent descendant-height pre-check and `finish`'s
  unmarked-master check (`Candidate::apply` makes neither check; the delta rules report what full validation does).

### Survivors, with consumer

- `Candidate`: admission (`ProjectState::admit`), asset/font import (`assets.rs`, `postgres/resources.rs`), bundle
  validation, `Project::persist`, and the test oracles.
- `validate::incremental` (`validate_after_indexed`) and storage `batch/validation.rs`: test-only database oracle
  (`#[cfg(test)]`); phase 4.
- Moka, `RevisionIndexes` and the `scoped/` pieces listed under phase 1: unchanged, phase 4.

### Delta rules (`crates/scene-input/src/validate/delta/`)

Order as in `Candidate::validate`: node cap (in `finish`) → notes → navigation → per theme styles then nodes →
animation triggers → instances.

| Rule | Seeds / recheck set | Index |
| --- | --- | --- |
| Closure | Written nodes whose kind, parent or props changed (sort-key-only renumbers are no seed); deleted nodes; users of every changed token and of the tokens aliasing it (transitively); users of changed styles and of styles using a changed token; nodes scoped to a theme no token declares any more. Every member adds the instances of each component master containing it (both revisions, ≤ 128 ancestors). A member descends to its children only when it is new, moved, changed `width`/`height`/`layout`/`theme`/`component`, uses a changed token inside `width`/`height`/`layout`, or names a vanished theme | `referencing`, `aliasers`, `component_users`, `declared_themes` |
| Notes | Note count ≤ 5,000; every note the batch wrote (per-op checks and delete cascades cover the rest) | `note_count` |
| Links | Written nodes whose props changed and carry `links`, plus nodes linking to a node the batch created, moved (kind or parent changed) or deleted | `incoming_links` |
| Flows | When a written node is a root frame with `flowStart`: uniqueness over all root frames (O(root frames)) | `pages`, children |
| Properties per theme | Changed styles and styles reached from changed tokens, then the closure's nodes in id order (`Resolver::node`) | – |
| Animation triggers | Closure nodes with `animations[].triggerNodeId` | – |
| Component graph | Masters around the closure: cycle (`INSTANCE_CYCLE`) and depth (`INSTANCE_DEPTH`) by DFS from them | `component_users` |
| Instance expansion and budget | Expand the closure's instances (children in id order, as the reference); expanded total from `Facts` ≤ the engine's `Limits.max_nodes` (`INSTANCE_LIMIT`) | `Facts` |
| Instance checks | Per closure node: `component` shape, instance keys, master is a component frame, override targets/keys/props per theme, in-component links/animations/`flowStart`; then properties of the expanded nodes per theme | – |

Deviations from the design brief, all covered by the oracle and the scenario tests: style users and `styleId` changes
do not descend (style values hold only fill and text properties); token users descend only for `width`/`height`/
`layout`; link sources are limited as in the table instead of every closure node; the notes rule re-checks the notes
the batch wrote.

### Oracle results

- State differential (`Txn` + delta rules against `Candidate::apply` + `validate()`): seeds 1–8 plain, 1–16 with
  property ops, 201–212 with the new cases mix: 2,400 + 4,800 + 3,600 batches (731, 972 and 957 accepted).
  **0 differences**; the phase 2 `after_token` allowance is gone.
- `begin` differential: plain seeds 1–24 (7,200 batches), property seeds 101–116 (4,800), cases seeds 201–224 (7,200).
  Accepted 2,078, 895 and 1,774 (a fifth of each dropped to exercise rollback). **0 differences** in accept/reject,
  rows, changed ids, revisions and per-op code/field/op index; 0 end-of-batch order differences.
- The cases mix (`support/random_cases.rs`, `random_scripted.rs`) adds instances with overrides, hover animations with
  triggers, dimension and color tokens with theme branches and aliases used in sizes and wrapping rows, theme-scoped
  instances, edits inside masters, and scripted batches: subtrees moved past depth 128 and back, links to frames
  created later in the batch, masters unmarked and marked again, several end-of-batch errors.
- `tests/delta_rules.rs` and `tests/delta_structure.rs`: one scenario per rule and dependency path (size tokens through
  aliases and theme branches, theme scopes and vanished themes, styles, token type changes, unmarked masters, override
  targets, nested masters, instance cycles, triggers, links, flows, depth, the note count), each on the state and on
  the reference.

### Measurements

Machine: 10 cores; load average 2.3–3.5 during every run below (two rounds each, round 1 shown, a range where the
rounds differ by more than 10%).

**PostgreSQL** (`node_memory_writes_bench`, release, `--features large-project-bench`, temporary cluster), 30 measured
runs after two warm-ups, median / p95 in ms; "validation" is `memory.begin` (`Txn` + delta rules):

```bash
WRITES_BENCH_NODES=200000 scripts/task19-check.sh cargo test --release -p fagent-storage \
  --features large-project-bench --test node_memory_writes_bench -- --ignored --nocapture   # also 7000
```

| Case | 200k fixture (178,604 authored) | validation | persist | ~7k fixture (6,376 authored) | validation |
| --- | ---: | ---: | ---: | ---: | ---: |
| Small props edit (plain text node) | **2.9–4.3 / 4.9–5.4** | **0.011–0.013** | 2.2–3.2 | 1.04 / 1.32 | 0.009 |
| Master edit (`master_0`, 929 instances; 29 at 7k) | 21.2–22.2 / 23.1–25.7 | 16.0–17.0 | 3.9 | 1.53 / 1.68 | 0.48 |
| Reparent (152-node section across root frames) | 5.3–5.7 / 5.9–7.2 | 0.75–0.79 | 3.4–3.6 | 1.63 / 1.80 | 0.55 |
| Token edit (color used in `master_1`, 890 instances; 26 at 7k) | 20.7–21.8 / 21.8–23.4 | 15.4–16.6 | 3.9–4.0 | 1.54 / 1.58 | 0.46 |
| Note create | 3.1–4.4 / 5.1–5.4 | 0.005 | 2.4–3.3 | 1.09 / 1.19 | 0.004 |
| Page create | 2.5–3.0 / 3.3–4.0 | 0.010–0.012 | 1.8–2.2 | 1.05 / 1.38 | 0.006 |
| Cold commit: evict, load, admission, commit | 1,119–1,191 / 1,159–1,244 | – | 10.1 | 47.6 / 53.7 | – |
| `get_design_context` `root_10`, scoped part | 0.83 / 0.97 (171 nodes) | – | – | 0.45 / 0.48 (213 nodes) | – |
| RAM per authored node (engine estimate) | 1,252 B | | | 1,229 B | |

- Phase breakdown of the master edit at 200k (round 1, ms): `delta.closure` 0.66 (930 nodes), `delta.themes` 2.65,
  `delta.triggers` 0.04, `delta.instances.graph` 0.25, `.expand` 1.74 (3,716 expanded nodes), `.nodes` 2.29,
  `.properties` 7.28; the token edit is within 5% of it. The small props edit, note and page create spend under
  0.01 ms in every `delta.*` phase; the time is persist, the history envelope (0.01–0.03) and the database round
  trips.
- Cold commit at 200k: database load 171, `memory.load` 910 (admission `Candidate::validate` 825, of which
  `validate.theme` 388 and `validate.instances` 279; `memory.admission.facts` 21), persist 10. At 7k: load 15,
  admission 23, facts 0.6.
- `get_design_context` here is the storage part of the server read: the memory view at the database revision
  (identity probe), the frame closure with instance definitions and resolution under the default theme. The server's
  JSON shaping and `viewports` are not included.
- Comparison with phases 1–2: their "small props edit" request renamed the first non-instance child of a root frame
  in fixture order, which is `master_0_title`, a master child. It is the "master edit" row here: **31.8 → 21.2 ms**
  total, `memory.begin` 26.1 → 16.0 ms. A plain node now validates in about 0.01 ms.

**Engine** (`cargo run --release -p fagent-node-memory --example bench -- --runs 101 --load-runs 7 --spike a`, 200k
fixture, 178,415 authored nodes). The bench cap now sits above `MAX_NODES` (the fixture expands to exactly 200k and
the engine cap bounds the expanded project since this phase):

| Measure | Phase 2 | Phase 3 |
| --- | ---: | ---: |
| Footprint after load, per authored node | 1,594 B | 1,594 B (estimate 1,251 B) |
| Load + index build (no admission), median / p95 | 54.9 / 55.8 ms | 52.8–54.2 / 54.4–54.9 ms |
| Get node by id | 24 ns | 23–24 ns |
| Read: frame closure `root_10` (291 nodes) | 74–90 µs | 70–74 µs |
| Read: frame closure `frame_wide` (14,002 nodes) | 4.16–5.10 ms | 4.04–4.09 ms |
| Commit: 1 props update | 3.71 µs | 5.71–5.88 µs |
| Reject: 1 props update | 3.58 µs | 5.54–5.67 µs |
| Commit: create node (append) | 3.96 µs | 5.46–5.54 µs |
| Commit: create at index 0 of wide frame (14k renumbered) | 5.01 ms | 7.58–7.93 ms |
| Commit: reparent subtree across root frames | 7.38 µs | **443–448 µs** |
| Commit: delete root frame subtree (328 nodes) | 536 µs | 608–617 µs |
| Commit: 100 props updates in one batch | 193 µs | 183–185 µs |

Phase 2 engine rows ran structural and per-op checks only (end-of-batch validation sat in `HotProject::begin`); phase 3
rows include the delta rules. The reparent now re-checks the moved subtree (152 nodes) under every theme; the
renumber pays the closure walk over 14k written rows, which are no seeds. Default bench (no `--spike`): begin + commit
of one update 2.7 µs, reparent of a wide frame across pages 22 ms.

**Target:** validation of a small props edit under 5 ms — **met**: 0.011–0.013 ms on PostgreSQL at 200k (5 µs in the
engine bench), commit total 2.9–4.3 ms median. Edits that reach ~900 instances validate in 15–17 ms.

### Verification

| Suite | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| scene-input + node-memory | 162 | 0 | 2 |
| SQLite storage | 234 | 0 | 40 |
| Server | 358 | 41 | 12 |
| PostgreSQL default (`scripts/task19-check.sh`: storage 19 + server `postgres_backend` 2) | 21 | 0 | 0 |
| PostgreSQL targeted (memory writes/reads, revision indexes, scoped reads, dry run, write-through, freshness) | 12 | 0 | 0 |

The 41 server failures are the same environmental set as in phase 2 (no renderer build in the worktree: render worker
`WORKER_UNAVAILABLE` and its downstream checks, `startup_env` refusing the Chromium export backend, `esbuild`
missing); storage + server total 592 passed as in phase 2. fmt and `clippy --all-targets -D warnings` are clean on
scene-input, node-memory, storage (also with `--features large-project-bench`) and server.

### Behavior changes

- Pure diff-only validation on stored invalid data: a stored defect rejects a batch only when the batch's closure
  reaches it. Phase 2 blocked commits on project-wide defects through the reference path (notes, navigation, triggers,
  instance graph), on a dangling token alias (whole token set validated) and on a broken `componentId` (every instance
  expanded). `tests/admission.rs` now expects unrelated batches on such bases to be accepted.
- Token upserts are checked right after each upsert, exactly as `Candidate::apply` does; a batch that leaves the token
  set invalid mid-batch is rejected at that op even when a later op repairs it. The public result is unchanged
  (phase 2 got there through the reference path); only the cost changes, O(affected tokens) per upsert.
- The expanded-node limit follows the engine's `Limits.max_nodes` (`MAX_NODES` by default), so a raised engine cap
  also raises the expanded cap. The engine bench now sets its cap above `MAX_NODES`.
- `ProjectState::apply` alone also runs the end-of-batch rules (callers of the state outside `NodeMemory` get the full
  batch validation).
- Error order among several end-of-batch errors may differ from `validate()` (allowed by the task); none showed up
  in the oracle runs.

### Open items

- Edits that reach many instances cost per instance: ~16 ms of validation for ~900 instances at 200k, mostly
  `delta.instances.properties` and `delta.themes`. Phase 5 (lazy expansion cache) or caching resolved master
  properties would cut it.
- The reparent of a subtree re-checks every moved node under every theme (~0.45 ms per 150 nodes); a wide-frame
  renumber walks every written row in the closure (7.6 ms for 14k rows, was 5 ms).
- Admission still costs one full `Candidate::validate` per load (825 ms at 200k, half of it `validate.theme`).
- Not measured: the server-side shaping of `get_design_context` (JSON, viewports), `get_design_context` right after a
  write, and the real-data restore (`WRITES_BENCH_REALDATA_URL`, not available here).
- Phase 4 (Moka, unused `RevisionIndexes`/`scoped/` code, test-only `validate_after_indexed` and `batch/validation.rs`,
  `docs/REJECTED-APPROACHES.md` lines) is not started and waits for the user's go-ahead.
