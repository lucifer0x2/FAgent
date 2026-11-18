# Task 41 — Self-contained node-memory engine, always on, diff-only validation

**Priority (2026-11-18):** requested by the user after the Task 28 commit-path review; not yet placed in the order
34 → 36 → 37 → 31/35 → 33.
**Status:** Phase 1 done (2026-11-18, `feat/task-41-node-memory-engine`); spike 1b done (2026-11-18, `spike/task-41-1b`), user chose the radix trie (c); phase 2 done (2026-11-18, `feat/task-41-phase-2`); phase 3 done (2026-11-19, same branch: delta rules, classifier and reference path removed; see `docs/TASK41-REPORT.md`); phase 4 not started, waits for the user's go-ahead. Phases run in order; each phase ends with a report to the user, and the next phase starts only
after the user's go-ahead.
**Depends on:** Task 28 code as it is on `main` (I0–I5 implemented; I4/I5 deep tests and benchmarks were deferred).
Supersedes the open parts of Task 28 (flags, shadow modes, I5 cleanup) once this task is accepted.
**Expected owned scope:** `crates/node-memory` (the engine), `crates/scene-input/src/validate` (validator lookup trait,
per-op checks, delta rules), `crates/storage` (adapters: `shadow*`, `batch*`, `postgres/`, `backend/sqlite*`,
`hot_cache*`, `revision_indexes*`, `scoped*`), `crates/server` only where a read still needs a full snapshot, and the
docs listed under "Docs". Do not touch the viewer, renderer, export backends or the MCP tool shapes.
**Workspace:** new branch from `main`.

## Decisions (user, 2026-11-18)

1. **One struct, self-contained.** All in-memory project logic lives behind one public struct in `crates/node-memory`
   (working name `DesignMemory`; final name decided in phase 1). It knows nothing about databases: no SQL crates, no
   `SnapshotCacheIdentity`, no incarnation/file stamps, no storage types. Storage is an adapter that loads snapshots
   into it and persists the rows it returns.
2. **Always on, no options.** No `FAGENT_NODE_MEMORY_SHADOW/READS/WRITES`, no `off`/`on`/`strict` modes, no shadow
   mirror in production. Every latest-revision read and every batch goes through the engine.
3. **Warm | cold is the architecture.** A cold project is loaded from the database on first access and fully validated
   once (admission). A slow first access is acceptable.
4. **Diff-only validation per commit.** A commit never revalidates the whole project. Given a valid base, the batch's
   delta plus its dependency closure is checked; whole-project passes are replaced by incremental rules over live
   indexes.
5. **Per-op checks stay.** Each create/update/reparent is checked right after it is applied, as `Candidate::apply`
   does today (`crates/scene-input/src/validate/candidate.rs:148`), so a batch that passes through an invalid
   intermediate state is still rejected at that operation index. Batch-level rules (navigation, flows, triggers,
   totals) run once at the end of the batch, because a batch may link to a frame created later in it.
6. **Error order may change.** Accept/reject must match today's validator exactly. When a batch has several
   end-of-batch errors, which one is reported first may differ; document it. Per-op errors keep their op index.
7. **No pre-expanded instances.** Authored nodes stay the only source. Keep per-master expanded-size counters for the
   node limit; expand virtual nodes only for reads.

## Context (code of 2026-11-18)

- `crates/node-memory` already depends only on `fagent-scene-input`, `serde` and `serde_json`. It holds the slot arena,
  live indexes (ids, sorted children, pages, component users, incoming links, note anchors), per-prop write stamps,
  `Txn` with an undo log, two-phase `Staged` commit and the persistent `ReadView` publication
  (`state/staged.rs`, `state/publication.rs`, `registry/handle.rs`). It runs structural checks only.
- The rest of the in-memory logic sits in `crates/storage/src/shadow.rs` and `crates/storage/src/shadow/` (about 2,300
  lines including tests), mixed with database concerns: env flags (`read_mode.rs`, `write_mode.rs`, `shadow.rs`),
  incarnation tags bound to `SnapshotCacheIdentity` (`reads.rs`), the mirror/compare path (`mirror.rs`, `compare.rs`),
  `memory_handle`/`memory_base` and the I4 local batch (`incremental.rs`, `writes.rs`), counters (`stats.rs`).
- HOT commit (I4), SQLite `crates/storage/src/batch/incremental.rs`, PostgreSQL `crates/storage/src/postgres/batch.rs`:
  probe identity → `memory.prepare` → `classify::supports` → `stage` → `validate_delta` (affected + support closure,
  small `Candidate`, op replay to keep error precedence) → preimages/footprint → strict page guard → persist touched
  rows → `staged.commit()`. The write lock is held from stage to commit.
- Fallback commit (any op the classifier rejects: page create/edit, root-level structural ops, tokens, styles, notes,
  props `theme`/`links`/`flowStart`/`animations`; `crates/scene-input/src/validate/memory/classify.rs`):
  `to_snapshot()` or a database load → `Candidate::from_snapshot` → `validate_after_indexed`
  (`crates/scene-input/src/validate/incremental.rs`) → scope/history → persist → `cache.fill`.
- Why `validate` stays at ~210 ms on the 200k fixture even though only the changed node is property-checked: the
  incremental validator still rebuilds `NodeIndex` before/after, scans every node for notes, navigation and animation
  triggers, and expands every instance of the project on every commit. Task 25 (PG 30×, strict, median ms):
  `candidate.validate` 1,196.5, of which `validate.instances` 1,119.4 and `validate.instances.expand` 753.7;
  `validate.notes_navigation` 39.7; per-theme property checks 0.03–0.05 (`docs/TASK25-REPORT.md`).
- `validate_delta` needs a certificate on the base view; the first commit after a load runs a full `validate()` and
  `instances::expand` to create it (`crates/scene-input/src/validate/memory.rs:45`).
- Reads already served from memory: scoped `get_design_context`, `get_subtree`, `get_outline`/`find_nodes`, project-wide
  outline/find, `list_*`, `get_edit_context`, prerender (`crates/server/src/reads/memory.rs`, `docs/TASK28-I5-REPORT.md`).
  Still on full snapshots: explicit `revision` reads, export/bundle, `get_changes`, HTTP full snapshot.
- PostgreSQL keeps a Moka snapshot cache (`crates/storage/src/hot_cache.rs`, TTL 1 h, 64 MiB) filled after each commit
  (`postgres/write_through.rs::fill`); for HOT projects `fill` already only evicts. `RevisionIndexes` and
  `crates/storage/src/scoped/` serve the fallback validator and the strict page guard.

## Target design

### Engine boundary (`crates/node-memory`)

One public struct owns every HOT project and exposes a small API; sketch, names final in phase 1:

- `load(snapshot) -> Result<Admission>`: index the project and run the full admission validation once. The result says
  whether the stored data is valid (see "Admission on invalid stored data").
- `view(project) -> Option<Arc<ReadView>>`: latest published revision; never blocks on writers.
- `begin(project, ops) -> Result<Staged>`: apply ops under the project write lock with per-op checks, then the
  end-of-batch delta rules. `Staged` exposes the revision, `Delta`, rows to persist (with preimages), changed ids,
  history footprint (`node:`/`page:`/`note:` keys) and both views for guards.
- `Staged::commit()` publishes; dropping it rolls back exactly.
- `evict(project)`, `expire()` (TTL and byte budget), counters for loads, admissions, commits, rollbacks, evictions.
- `to_snapshot(project)` for full-snapshot consumers of the latest revision.

The engine never reads the clock or environment directly beyond the existing `Clock` trait, never spawns tasks and
never knows where a snapshot came from.

### Storage adapter (`crates/storage`)

- Cold access: load the project's rows (`entities_for_write` or the SQLite equivalent) → `engine.load`.
- Freshness stays outside the engine: under the existing writer lock / `FOR UPDATE` row lock, compare the database
  revision and incarnation with what the adapter loaded; on mismatch evict and reload (several processes may share one
  PostgreSQL). The adapter keeps its own `project → identity` map; the engine stores no identity.
- Commit: idempotency replay → freshness check → `engine.begin` → strict page guard and history coverage (database
  history, engine views) → persist the returned rows, idempotency and history in one transaction → `commit()`; on
  database error drop `Staged` and evict.
- Non-batch writes (asset/font import, project meta, bundle import/restore, retention, migrations) evict; the next
  access reloads.
- Removed: env flags and modes, shadow mirror/compare in production code, `memory_base`/`to_snapshot` commit base,
  `Candidate::from_snapshot` on the commit path, `cache.fill`, and the Moka cache, `RevisionIndexes` and
  `scoped/` pieces that have no consumer left (list every removal and every survivor with its consumer).

### Validation over engine state (`crates/scene-input/src/validate`)

- Introduce a read-only lookup trait for validators (nodes by id, parent/children, tokens, styles, fonts, assets,
  notes, component users, incoming links). `Candidate` implements it for bundle import and as the test oracle; the
  engine implements it over its live state. `Resolver` and the property checks read through it instead of borrowing
  `Candidate`'s maps.
- Per-op (inside `Txn`, after each create/update/reparent): structure (page roots, container parent, no cycle,
  depth ≤ 128) and `properties` of that node under the current theme, exactly as `Candidate::apply` today.
- End of batch, on the delta and its dependency closure only:

| Rule | Recheck when | Index |
| --- | --- | --- |
| Property resolution under every theme | Node changed; node gets a new effective theme (`theme` set/removed on an ancestor, reparent across theme scopes); a token or style it uses changed | token → users, style → users (new) |
| Tokens: types, alias target same type, no cycle, ≤ 4096 tokens, ≤ 32 themes | `UpsertToken` | alias graph from the written token; counters (new) |
| Styles: kind and value rules | `UpsertStyle` | style → users (new) |
| Notes: anchor exists, id not a node id, text/author/replies limits, ≤ 5000 notes | Note op; anchor node deleted (cascade) | note anchors, ids (exist); counter (new) |
| Links point to a root frame | `links` written on X; frame F reparented or deleted (delete prunes, no error) | incoming links `store.linking()` (exists) |
| Flow names unique | `flowStart` written/removed; frame becomes or stops being a root frame | flow name → frame (new) |
| Animation trigger is a strict group/frame ancestor in the same root frame | `animations` written; trigger node reparented or deleted | trigger → targets (new) |
| Component graph has no cycle | Instance created/reparented inside a master; `componentId` or `component` changed | component users (exists); DFS from the edge |
| Instance keys, master is a component frame, override targets are master descendants, override props valid | Instance changed; its master subtree changed | component users (exists) |
| Authored nodes ≤ `MAX_NODES`; expanded nodes ≤ `MAX_NODES` | Any create/delete; master subtree size changed | authored count; per-master expanded size, propagated through component users (new) |
| Root frame size, image asset kind, font/asset references | Node changed; asset/font removed (non-batch write evicts) | ids (exists) |

- Admission at `load` uses the full `Candidate::validate()` once (or the trait-based equivalent) and initializes the
  counters and new indexes.
- `classify::supports`, the op replay in `validate_delta` and the whole-project fallback are deleted once every op class
  above is covered.

### Reads

- Latest-revision reads all come from the engine view. Full-snapshot consumers of the latest revision use
  `to_snapshot`. Explicit past-revision reads keep their database path (Task 39 owns past revisions).
- Phase 5 (optional, only if measurements justify it): a lazy expansion cache per instance, keyed by the master
  subtree's write stamps and invalidated through component users.

## Phases

1. **Boundary.** Create the engine struct; move registry, views, staging, local-batch preparation, counters and TTL /
   budget handling out of `crates/storage/src/shadow*` into `crates/node-memory`. Storage keeps identity and
   freshness. Remove the env flags and modes: memory is always on. Unsupported op classes still validate inside the
   engine with a full validation built from engine state (no database load, no `cache.fill`). Behavior otherwise
   unchanged.
2. **Validator trait and per-op checks.** Lookup trait, `Candidate` and engine implementations, per-op checks in `Txn`,
   admission at `load`.
3. **Delta rules.** Implement the table above, new indexes and counters; delete the classifier, op replay and the
   engine-internal full fallback. `Candidate` leaves the commit path.
4. **Cleanup.** Remove the Moka cache, `cache.fill`, unused `RevisionIndexes`/`scoped/` code and remaining shadow code;
   route remaining latest-revision full-snapshot reads through the engine; update docs. Add one line per dropped
   approach to `docs/REJECTED-APPROACHES.md` (shadow mirror, opt-in flags, whole-project per-commit validation,
   latest-revision snapshot cache).
5. **Optional lazy expansion cache** (see Reads).

## Acceptance criteria

- `crates/node-memory` builds without `crates/storage`, `rusqlite`, `postgres` or `moka`; `rg` finds no
  `FAGENT_NODE_MEMORY_` outside `docs/REJECTED-APPROACHES.md` and closed reports; no `strict` node-memory mode remains.
- Commit path contains no `to_snapshot`, `Candidate::from_snapshot`, `instances::expand` over the whole project or
  whole-project note/navigation/trigger scans (check by code reading and by the bench phase counters).
- Differential oracle: seeded random batches over generated projects covering every op kind (pages, tokens with themes
  and aliases, styles, notes, links, flows, animations with triggers, nested components, overrides, deletes, invalid
  intermediate states repaired later, batches with several errors). Against `Candidate` (apply per op, then full
  `validate()`): identical accept/reject; on accept identical rows, sort keys, resources, notes and changed ids; on a
  per-op rejection identical code, field and op index; on an end-of-batch rejection identical accept/reject and, when
  only one rule fails, identical code and field. Report seeds, batch counts and every difference found.
- Storage and server suites pass on SQLite and on PostgreSQL (temporary cluster via `scripts/task19-check.sh`); only
  the already known environment failures.
- Benchmarks, PostgreSQL release build, 200k fixture and one synthetic project of about 7k nodes, median of 30 with
  p95: small props edit, reparent, master edit with many instances, token edit, note create, page create, admission
  `load`, `get_design_context` one frame, RAM per authored node. Report the breakdown by phase; target for a small
  props edit: validation under 5 ms.
- fmt and `clippy --all-targets -D warnings` clean for touched crates; module rules from `AGENTS.md` (explain files
  over 1000 lines).

## Docs

`README.md` "Performance at a glance" (drop the flag note and the stale 468 ms row once phase 3 numbers exist),
`docs/PERFORMANCE.md`, `docs/ARCHITECTURE.md`, `docs/SERVER.md` (commit flow, error-order note), `AGENTS.md`
"Current status", `docs/agents/LOG.md`.

## Admission on invalid stored data (decided 2026-11-18)

When admission finds that stored data is invalid (rows written by an older validator or edited by hand): reads are
served; batches keep diff-only validation (today's incremental path has the same precondition); the admission errors
are counted and logged with project and node ids, never content. Implemented in phase 2. Final struct name:
`NodeMemory`.

## Outputs

Commits per phase, a short report per phase under `docs/TASK41-REPORT.md` (API, removals and survivors, oracle
results, benchmark tables, files over 1000 lines), log entries in `docs/agents/LOG.md`.
