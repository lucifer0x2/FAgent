# Task28 I4 — staged delta validation, 2026-11-16

Status: implementation complete with bounded checks; **full AC verification remains
open**. Work continued directly from `93d2381` on `feat/node-memory-writes` in the
existing node-memory worktree. The user deferred deep tests, full differential,
PostgreSQL restore/matrix and all benchmarks until implementation slices finish.
No I5, push, deploy, restart, live listener, native database or private-data access.

## Validation and publication

- `scene-input::validate::memory::{supports, validate_delta}` is a separate partial
  validation entry point. It receives exact staged changed IDs and old/new immutable
  `ReadView`s. A partial candidate is never passed to `Candidate::validate` as if it
  represented a complete project.
- A certificate belongs to an exact `ReadView` Arc/revision. Before first local
  validation, a full reference admission checks the entire loaded revision and records
  its exact expansion count plus persistent per-instance virtual-node weights.
  Structure-only library loads and legacy invalid rows cannot acquire incremental
  trust. Storage supplies the existing builtin identity/incarnation/schema/readiness/
  current-revision boundary. A failed admission takes the reference path so a batch
  that repairs legacy data can still work.
- Affected rows follow descendants, old/new ancestry and every enclosing master to
  instance users, transitively across nested masters and pages. Complete placement
  parent subtrees retain unchanged siblings; temporary parents and component targets
  that disappear from both endpoint revisions remain in the replay scope. Ancestors,
  navigation targets and complete referenced-master subtrees supply dependencies.
- A bounded preimage candidate replays operations using the existing `Candidate::apply`
  and `prune_links`, preserving intermediate validation, first operation error, public
  fields and suggestions. An invalid property repaired by a later operation still
  rejects at its original index. Delta/replay disagreement conservatively falls back.
  Final authored validation runs in sorted ID order across every declared theme;
  component graph/overrides/virtual properties reuse the existing validator on the
  bounded dependency closure.
- The certificate subtracts old affected instance weights and adds exact new weights
  from local expansion, including nested placements. The project cap includes unchanged
  instances outside the local closure, even when local expansion contains no instances.
- `Staged::preview()` derives an immutable successor from its own committed base.
  Storage validates that successor; durable DB success publishes the same Arc. It
  never exposes staged rows to readers. Failure/drop rolls back state/indexes/stamps;
  a DB failure also evicts the HOT project. Abort updates registry budget proxies when
  successful admission retained a certificate on the old published view.

## Storage adapters and fallback matrix

`shadow::incremental` prepares touched preimages/rows and exact node/note/page
footprints. SQLite `batch::incremental` and PostgreSQL's HOT branch persist those
rows in the existing transaction/history/idempotency path, then publish memory.
Ordinary HOT validation builds no full snapshot, project payload-position map,
full-project Candidate, PageIndex, RevisionIndexes or global instance expansion
after admission. Persistence's existing comparison helpers scan only indexed touched
preimages. `concurrent` still uses durable history and existing operation-write rules,
including token/style existence when their IDs overlap a deleted node ID.

| Class | Path |
| --- | --- |
| Plain node/root/master props, instance overrides/component target | Local delta validation |
| Create/delete/reparent/reorder inside non-page containers | Local; complete placement siblings/subtrees |
| Current-revision strict page guard on supported operations | Local source/destination/footprint page checks |
| Stale strict page guard | Existing full dependency/history guard adapter |
| Resource, note, page, explicit theme changes | Full reference |
| Explicit links, flowStart or animations writes | Full reference; preserve navigation/global/error contracts |
| Structural root/page ordering or root placement | Full reference; preserve default navigation/wildcard footprint |
| Lib rejection, invalid legacy admission, replay mismatch | Full reference; preserve acceptance/error precedence |
| Cold/stale/incarnation mismatch, flags off, custom/import/historical, dry run | Existing paths |
| `WritesMode::Strict` feature | Local adapter plus explicit full validation/error/changed-row oracle |

Feature strict mode is distinct from request `strict:true`. It intentionally retains
full work for later differential verification. The ordinary `On` mode performs local
work after admission. A new `writeIncremental` diagnostic counter counts accepted delta
validations; `writeFallbacks` now also includes pre-validation lib rejections routed
to the reference path, not only library rejections of already accepted candidates.

## Remaining costs and limits

Admission is full work once per uncertified loaded view and can occur on the first
HOT write. A full fallback publishes an uncertified successor, so the next supported
write must admit it again. Admission currently expands once for reference validation
and again to build exact weights. The entire admission cost is explicit in
`memory.validation.admission`; it is not a warm incremental-write latency claim.

Partial replay clones its affected/support node payloads and anchored notes; resource
vectors/maps and theme resolution still use existing representations. Moving/editing a
large container can touch its full subtree and master users; this is conservative
dependency work. Persistent maps retain I3b's 256-shard copy-on-write tradeoff.

PostgreSQL still fills the full snapshot cache after commit. A HOT commit retains its
before `ReadView`; `Committed::from_memory` defers conversion to the existing separately
timed `cache.fill` phase. This is real whole-project conversion/merge/order/cache work,
left for I5, with possible retained-view/cache memory overlap. Old guard/cache/index
layers remain for fallbacks. No claim that total writes reach 2–5 ms, that RAM meets a
target or that full backend parity has been verified.

## Bounded checks actually run

All cargo checks used the shared target and `CARGO_BUILD_JOBS=2`, sequentially:

```sh
export CARGO_TARGET_DIR=target
export CARGO_BUILD_JOBS=2
cargo check --locked --offline -p fagent-scene-input -p fagent-node-memory
cargo check --locked --offline -p fagent-storage -p fagent-server
cargo clippy --locked --offline -p fagent-node-memory -p fagent-scene-input -p fagent-storage --lib -- -D warnings
cargo test --locked --offline -p fagent-scene-input --lib memory_delta -- --test-threads=1
cargo test --locked --offline -p fagent-storage --test node_memory_writes sqlite_i4_delta_smoke -- --test-threads=1
cargo test --locked --offline -p fagent-node-memory --test publication -- --test-threads=1
cargo fmt -p fagent-node-memory -p fagent-scene-input -p fagent-storage
git diff --check
```

The initial scene/node check found an import mistake (corrected); initial storage
check found one unused export (removed). Final checks passed. Scene smoke: 5 passed
(local/cross-page reject parity, invalid prefix error, exact global cap with a small
synthetic near-cap certificate, invalid admission, two successive nested-master edits
with full reference expansion totals). SQLite smoke: 1 passed (accept/reject, strict
current page guard, stale LWW report, cross-page virtual validation, placement/replay,
forced SQL failure and rollback/recovery). Publication smoke: 2 passed. No PG service
was started. Shell fmt required normal sandbox escalation for the authorized sibling
worktree; no workaround or unrelated directory writes.

Product commits: `3494a02` (validation/publication), `10b861a` (storage adapters).

Modified Rust modules stay below 500 lines. The largest is existing PostgreSQL batch
adapter, still cohesive around commit/preview transactions. New modules separate
classification, closure traversal, validation/certification and storage adaptation by
responsibility. No >1000-line exception is needed. `AGENTS.md` unchanged: 72 lines /
9,921 bytes.

## Coordinator check on the committed branch

At `c29efe7` on `feat/node-memory-writes`, the coordinator independently reran:

```sh
CARGO_TARGET_DIR=target CARGO_BUILD_JOBS=2 cargo check --locked --offline -p fagent-storage -p fagent-server
cargo fmt --check -p fagent-node-memory -p fagent-scene-input -p fagent-storage
CARGO_TARGET_DIR=target CARGO_BUILD_JOBS=2 cargo test --locked --offline -p fagent-scene-input --lib memory_delta -- --test-threads=1
git diff --check 93d2381..HEAD
```

All passed; scene delta smoke 5/5. The source audit covered admission authority,
prefix errors, nested/cross-page dependencies, global expansion cap, durable
publication/rollback, footprints, strict guards and history-based concurrent reports.
All 22 changed Rust files are below 500 lines; largest is PostgreSQL batch at 277.
The coordinator did not broaden the deferred tests or run benchmarks.

## Deferred verification

Run later with temporary isolated data only; no native PostgreSQL or real data root:

```sh
CARGO_BUILD_JOBS=2 cargo test -p fagent-scene-input -p fagent-node-memory -- --test-threads=1
CARGO_BUILD_JOBS=2 cargo test -p fagent-storage -p fagent-server -- --test-threads=1
FAGENT_NODE_MEMORY_WRITES=strict FAGENT_NODE_MEMORY_READS=strict FAGENT_NODE_MEMORY_SHADOW=strict CARGO_BUILD_JOBS=2 cargo test -p fagent-storage -p fagent-server -- --test-threads=1
bash scripts/task19-check.sh
# The PG script's test database must be disposable; explicitly include the new smoke:
CARGO_BUILD_JOBS=2 cargo test -p fagent-storage --test node_memory_writes postgres_i4_delta_smoke -- --ignored --test-threads=1
WRITES_REALDATA_URLS='<restore-off-url>|<restore-on-url>' CARGO_BUILD_JOBS=2 cargo test --release -p fagent-storage --test node_memory_writes_realdata -- --ignored --nocapture --test-threads=1
CARGO_BUILD_JOBS=2 cargo test --release -p fagent-server --lib generated_5k_reads_from_memory_match_today -- --ignored --nocapture --test-threads=1
```

The new nested test is small; broad differential remains deferred, including rejection
ordering, overlapping resource IDs, all theme combinations, delete/recreate cascades,
strict stale guards, DB-failure modes, concurrent writers, cache fill/read identities,
both backend flags paths and actual PostgreSQL transaction behavior. Retained-view RAM,
peak admission/commit/cache overlap and 30-sample latency need later measured harness
work; existing ignored timing commands alone do not establish those ACs.
