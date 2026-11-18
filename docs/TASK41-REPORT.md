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
