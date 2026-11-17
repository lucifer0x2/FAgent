# Task28 I3b — persistent reader views, 2026-11-16

Status: implementation complete with bounded compile/smoke checks; **not fully verified**.
The user requested code focus and explicitly deferred deep tests and all benchmarks.
Work started from `2b81acd` in a disposable worktree; the coordinator transfers the final
commits to the existing `feat/node-memory-writes` branch. No I4/I5, live runtime changes,
deploy/restart, PostgreSQL restore, private-data writes or benchmarks in this slice.

## Boundaries and behavior

- `scene-input::read_scope::ReadView` owns one immutable committed revision. Payloads
  are `Arc<Node>`/`Arc<Note>` in persistent string maps. Reader-format indexes retain
  topology/roots/frames, page membership, note anchors, top-level component dependencies
  and instance users, exact navigation incoming edges, generic inverse references and
  outgoing node/token/style/font resource adjacency.
- `ReadView::advance` receives exact changed rows from the committed `Delta`. Plain
  node properties retain the same index Arc; reference changes scan only the touched
  row and replace changed memberships; creates/deletes/reparents update affected
  topology/page/anchor entries, including unchanged descendants of moved containers.
  Reorder/renumber updates payload sort keys; scoped ordering uses builtin storage's
  `(parent_id, sort_key, id)` order without rebuilding a project-wide positional map.
- `ProjectHandle::read_view` briefly clones the current Arc under a publication lock.
  It never takes the writer-state lock. `Staged::commit` builds/publishes its successor
  after storage's DB commit; abort/drop leaves the published Arc and budget unchanged.
  Readers retaining revision N do not hold any guard while using it, so cannot block
  staging or publication of N+1. Existing guarded `ProjectHandle::read` stays available
  for writer/diagnostic callers; storage HOT readers no longer use it.
- Storage's existing incarnation/schema/asset-readiness/current-revision probe still
  gates memory reads. Cold, stale, unknown target, flag-off, custom/untrusted and
  explicit historical reads keep their previous fallback/trust boundaries.
  Authority remains bound to the exact public metadata/index Arc pair and the view's
  exact metadata Arc; replacing a public Arc never grants persistent-view trust.
- A memory `ScopedRead.shared.snapshot` contains header/resources only. Consumers use
  `shared.scoped()` for nodes/notes. All existing HOT tree/find/context consumers use
  that interface; `node_count()` replaces a vector-length traversal bound. Public
  `shared_snapshot`/HTTP snapshots remain full. Explicit `shared.edit_context()` on
  this memory adapter has a full compatibility conversion; the server's existing
  get-edit-context route still uses today's full path, so ordinary HOT scoped reads
  never call that conversion.
- Existing snapshot-bytes identity keys and commit footprint/validation/result paths
  remain unchanged. A master edit therefore changes the revision identity and the
  next dependent cross-page instance reads its new master; no new resolved-result
  cache can retain stale instance payloads. Full writer `Candidate`/PageIndex/
  DependencyIndex construction and cache fill remain I4/I5 work.

## Sharing and accounting tradeoffs

No new dependency was needed: each persistent map has 256 shared BTreeMap buckets,
with copy-on-write only for changed buckets. Updating a bucket shallow-clones its
co-resident entries; Arc payloads and untouched buckets remain shared. This is a
bounded-shard tradeoff, not a claim of logarithmic persistent-tree update cost.
Root/frame sets copy on structural changes; source ordering sorts selected closures.
Resource vectors retain their existing Snapshot representation and are copied when
building a revision header; this slice removes project-node/slice copying on reads.

Map byte proxies update from touched pre/post values, so a plain-property commit does
not scan/serialize every node or every index for accounting. Registry budgets include
the current retained view payloads/indexes/header; old views survive only while callers
retain them. Estimates are conservative proxies, not measured RSS or bytes/node.

All touched handwritten Rust modules are <=250 lines; the largest is existing
`crates/storage/src/revision_indexes.rs` (246 lines). New modules split persistent map,
payload API, delta update, indexes, traversal and tests. No size exception is needed.
`AGENTS.md` remains unchanged: 71 lines / 9,957 bytes.

## Commands actually run

Working directory: disposable I3b worktree. Cargo commands used
`CARGO_TARGET_DIR=target CARGO_BUILD_JOBS=2` and ran
sequentially; no live services were started. Results:

```sh
cargo check -p fagent-storage -p fagent-server
# passed (7 crates)
cargo test -p fagent-node-memory --test publication -- --test-threads=1
# 2 passed: publication/abort/retained revision and small structural/reference sequence
cargo test -p fagent-server --lib committed_fixture_reads_from_memory_match_today -- --test-threads=1
# 1 passed: existing committed-fixture read differential, interleaved edits/virtual IDs
cargo test -p fagent-scene-input --lib view_tests -- --test-threads=1
# 3 passed: index Arc/payload sharing, exact master/navigation edges, retained resources
cargo clippy -p fagent-node-memory -p fagent-scene-input -p fagent-storage -p fagent-server --lib -- -D warnings
# passed
cargo test -p fagent-storage --lib memory_view_authority -- --test-threads=1
# 1 passed: exact authority binding and swapped/untrusted Arcs
cargo test -p fagent-node-memory --test registry budget_evicts -- --test-threads=1
# 1 passed: LRU/budget now includes the retained reader view
cargo test -p fagent-storage --test node_memory_reads sqlite_scoped_reads_come_from_memory_when_current -- --test-threads=1
# 1 passed: SQLite mirror publication, freshness, eviction and flag-off
cargo fmt -p fagent-node-memory -p fagent-scene-input -p fagent-storage -p fagent-server
git diff --check
# passed; fmt rerun after final edits
```

Later small changes tightened unchanged-reference Arc reuse and shared empty map buckets;
the coordinator performed final compile/fmt/diff verification after integration. No full
suite, ignored generated fixture, PG suite, real restore, timing or RAM run is implied.

### Coordinator integration check

Fast-forwarded the existing `feat/node-memory-writes` worktree to `2f540b2` (product
commits `3872f26`, `cc45e55`). From that worktree the coordinator independently ran:

```sh
CARGO_TARGET_DIR=target CARGO_BUILD_JOBS=2 cargo check --locked --offline -p fagent-storage -p fagent-server
cargo fmt --check -p fagent-node-memory -p fagent-scene-input -p fagent-storage -p fagent-server
CARGO_TARGET_DIR=target CARGO_BUILD_JOBS=2 cargo test --locked --offline -p fagent-scene-input --lib view_tests -- --test-threads=1
git diff --check 2b81acd
```

All passed; sharing smoke 3/3. Inspected all 27 touched Rust file sizes: none exceeds
250 lines. Four exceed the ideal 200: `scoped/mod.rs` 210 and `scoped/view_indexes.rs`
206 in scene-input; `revision_indexes.rs` 246 and `shadow/reads.rs` 215 in storage.
They remain focused on their respective capability/index/adapter responsibilities.
No deep tests or measurements were added to the coordinator check; no push/deploy.

## Deferred verification and measurements

Run only during the later validation phase, with temporary data and disposable PG
(never port 2703 or native data). Preserve flags-off and strict flags-on comparisons:

```sh
CARGO_BUILD_JOBS=2 cargo test -p fagent-node-memory -- --test-threads=1
CARGO_BUILD_JOBS=2 cargo test -p fagent-storage -p fagent-server -- --test-threads=1
FAGENT_NODE_MEMORY_WRITES=strict FAGENT_NODE_MEMORY_READS=strict FAGENT_NODE_MEMORY_SHADOW=strict CARGO_BUILD_JOBS=2 cargo test -p fagent-storage -p fagent-server -- --test-threads=1
CARGO_BUILD_JOBS=2 cargo test --release -p fagent-server --lib generated_5k_reads_from_memory_match_today -- --ignored --nocapture --test-threads=1
bash scripts/task19-check.sh
MEMORY_DIFF_REALDATA_URL='<disposable-restore-url>' CARGO_BUILD_JOBS=2 cargo test --release -p fagent-server --lib memory_diff::realdata -- --ignored --nocapture --test-threads=1
WRITES_REALDATA_URLS='<restore-off-url>|<restore-on-url>' CARGO_BUILD_JOBS=2 cargo test --release -p fagent-storage --test node_memory_writes_realdata -- --ignored --nocapture --test-threads=1
# Existing timing hooks, also deferred:
MEMORY_DIFF_NODES=200000 CARGO_BUILD_JOBS=2 cargo test --release -p fagent-server --lib generated_200k_frame_context_timing -- --ignored --nocapture --test-threads=1
```

The existing timing helper collects 15 warm and 7 post-write samples, so its command
alone does **not** satisfy I3b's median-30 AC. A later validation change must first
parameterize it for 30 samples and add isolated retained-index RAM/node measurement
(including old readers and load/commit peaks). No exact RAM/node AC command exists yet;
the estimate API must not be reported as such a measurement. Cross-page master edits,
resource/note deltas, eviction/reload, SQL failure/rollback and both flags paths belong
in that broader differential run. These deferred ACs remain open.
