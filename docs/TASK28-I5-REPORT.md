# Task28 I5 — HOT reader/cache cleanup, code complete, 2026-11-16

**Status: I5 code complete within the authorized code-focused scope; deep verification remains open.** After the earlier partial checkpoint, the user specifically approved the exact guard and reader artifacts ("handle the last 2 patches"). A new one-task subsession applied both through the dedicated patch tool and normal review, then added bounded differential and actual SQLite dispatch/rollback smokes. No rejected action was bypassed. This report does not claim all measurement/deep-test AC, read <1 ms, a RAM target, PostgreSQL runtime parity or the optional N7 heap reduction.

Work continued directly from `78cfdb0` on `feat/node-memory-writes` in the existing node-memory checkout. The coordinator integrated the original worker's cache wiring artifact after normal approval. Completion resumed from clean `2439fc4` in a new implementation subsession after the user's specific approval of the remaining guard/reader patches. No new branch/worktree, push, deployment, restart, live listener, native/private database or data-root access, generated workload, restore or benchmark.

Earlier product/test checkpoint: local commit `fa842f4` (44 Rust files), followed by the partial-status documentation through `2439fc4`. The completion diff contains the two exact remaining artifacts, formatting, focused tests and this status update; the cache artifact was not reapplied.

## Representation and reader boundaries

The selected representation keeps every authored property in its existing `serde_json::Value`. `NodeProps` provides borrowed typed access to name/text/component metadata/componentId/link targets, consumed by node-memory's master/component/link helpers, persistent index loading and server instance/search readers. It supplies no normalization or new validation defaults. Stored JSON, unknown keys, nulls, merge behavior and round trips stay unchanged. Packed typed props and string interning from optional N7 are **not implemented**; this accessor boundary is not a heap-reduction claim.

`ReadView` now retains sibling/root ordering as `(sort_key,id)` sets and frame source ordering as `(parent_id,sort_key,id)` sets, updated from the exact changed pre/post rows. Sort-only edits update the ordered keys without rewalking descendant page membership. Unchanged payloads/index shards and old readers retain the I3b/I4 sharing/publication behavior. Frame-key proxy accounting increases for the added parent ID; estimates remain proxies, not allocator/RSS measurements.

The server's `TreeIndex` can borrow `ScopedSnapshot` directly. Authored node lookup, child counts, ancestry and preorder use the capability instead of rebuilding an authored ID HashMap/child index. HOT adjacency is already ordered; conservative source-vector paths retain their stable sort behavior. Outline/subtree serialize requested authored nodes directly, expanding only instance child-count output where needed. Context uses the borrowed authored tree and indexes its requested expanded/resolved output separately. Instance-free find borrows authored nodes; mixed/virtual find retains its scoped expansion index and exact total scan. Navigation incoming sources come from the persistent incoming-link index and are ordered by ancestry paths for the requested edges, rather than indexing the full authored project.

Project-wide outline/find and MCP `list_tokens/styles/assets/fonts` now use the same latest read capability. Resource lists serialize only their own resource vector. Targeted `get_design_context` viewport preparation binds the same revision/resources. A persistent header deliberately has empty nodes/notes; `Prepared::shared` now selects note intent through the capability before its empty check. Full-vs-HOT preparation smoke covers input, key, resources, revision and overflow intent.

## Ownership, cache and fallback matrix

| Consumer/path | Accepted behavior |
| --- | --- |
| Current targeted/project-wide outline/find, subtree/context, MCP resource lists; READS on (or WRITES implies reads), exact fresh identity | Immutable persistent view; authored maps/child sorting are not rebuilt. Output-specific expanded/resolved indexes remain when required. |
| READS strict verification | Memory response plus existing full reference read/oracle; full reference work is intentional and not retained as a duplicate HOT cache entry. |
| PostgreSQL supported HOT write | Durable transaction and I4 publication unchanged; post-commit envelope retains metadata/touched rows, not the old full view. `cache.fill` exits before reconstruction/ordering/serialization/admission. |
| READS-enabled HOT project, full fallback write/read | Legacy snapshot/index objects may exist transiently to serve the requested fallback; owner policy prevents duplicate legacy cache retention/fills. |
| Initial admission / first observed HOT transition | Legacy Moka snapshot entry and all legacy revision-index identities for that project are invalidated once. A flag on the existing weak-handle tag records cleanup; retagging the same handle preserves it, reload gets a new flag. No new unbounded ownership registry. |
| Late read/commit cache fill | A brief cache admission gate plus ownership recheck suppresses it. Expensive snapshot weighting/index construction happens outside the gate so unrelated cold projects do not queue behind full-project work. |
| SHADOW-only, READS/WRITES off | Existing legacy cache/read behavior. A mirror entry alone does not establish HOT reader ownership. |
| Cold/stale/unknown target, failed memory read, external revision/incarnation/readiness change | Existing authoritative probe/fallback/reload behavior. Ownership policy can conservatively suppress a legacy cache while a stale handle is resident; it never authorizes serving stale payloads. |
| Explicit `snapshot`, `shared_snapshot`, HTTP full snapshot, export/bundle, explicit-revision reads | Complete existing full representation, ordering/readiness/retention contract. No implicit full API was changed to return a header. HOT legacy caches do not retain that full materialization. |
| MCP get_edit_context | Latest reader capability and persistent dependency closure; the exact existing wire formatter is shared with legacy (including 100-key summary/truncation). |
| Domain post-commit prerender, direct MCP/HTTP scenes, HTTP font inventory | Latest reader capability; node/frame/intent consumers use scoped access, font metadata uses the complete header/resources. Prepared scene input/key/revision/resources and note intent remain consistent. |

Identity/revision/incarnation/schema/readiness probes, TTL/LRU registry accounting, external-write/import/restore evictions, old-reader isolation, certificate authority and durable-before-ACK publication remain unchanged. Cache gates guard only brief admission/invalidation/lookup, not database work, full serialization or index construction. Moka invalidation is immediately respected by lookups; physical reclamation follows its normal maintenance and any in-flight Arc lifetimes.

## Guard implementation and approval history

`ReadView::dependency_closure` is a read-only adapter over persistent page/resource/reference/note adjacency. It expands only visited page output where placed props/effective fonts are required. A small oracle compares exact key sets and public errors with frozen legacy across invalid/duplicate/multiple pages, aliases/themes/styles, nested components and overrides, effective bundled fonts, links and anchored notes. It is now wired into supported HOT strict-page guards and get_edit_context.

SQLite and PostgreSQL supported HOT stale strict-page writes check existing pages, source/destination pages, exact affected-page footprint, both old/new dependency closures, history length <=1000, continuous revisions, scope version and sealed coverage. Gaps, invalid coverage, wildcards and dependency intersections conflict. Current-revision guards keep the same restrictions. Global/resource/note/root-order and invalid admission classes retain full fallbacks; LWW/concurrent reports, strict requests without pages, idempotency, durable-before-publish and rollback retain their contracts. PostgreSQL dispatch compiles but was not run against a PostgreSQL service here.

Exact worker-authored artifacts applied after the user's specific approval:

- `/private/tmp/fagent-i5-pending-guard.applypatch`: retain all strict checks and replace only supported HOT dependency/index dispatch; share the existing edit-context wire formatter; keep unsupported/global/resource/note/root-order fallbacks.
- `/private/tmp/fagent-i5-pending-readers.applypatch`: use the explicit capability in domain prerender, direct MCP/HTTP scene and HTTP font metadata consumers that already have scoped preparation helpers; leave full snapshot/export/bundle APIs complete.
- `/private/tmp/fagent-i5-pending-evidence.md`: exact scope, path/hunk descriptions, bounded oracle evidence and original worker rejection messages. The cache artifact listed there was subsequently approved/integrated; the two artifacts above were rejected again at coordinator integration.

Original worker rejections for these remaining integration areas, verbatim; coordinator integration also rejected both patches after the evidence-backed retry:

> This patch changes core commit concurrency and page-guard validation, including removing an existing strict-revision fallback, creating significant risk of stale writes or integrity regressions that the I5 authorization does not explicitly approve.

> This rewires MCP, scenes, jobs, domain, and font callers from full snapshots to a potentially metadata-only reader capability, risking incomplete nodes/resources and violating the requirement to keep implicit full consumers complete.

The user subsequently gave concrete approval for both artifacts, after being shown their links and the rejection reasons. Both dedicated patch calls then succeeded through normal review. No policy/configuration changes, alternate execution route or review bypass was used. The earlier rejection text above records the historical partial checkpoint, not a current blocker.

## Checks at the earlier partial checkpoint

All cargo checks ran sequentially with:

```sh
export CARGO_TARGET_DIR=target
export CARGO_BUILD_JOBS=2
cargo check --locked --offline -p fagent-storage -p fagent-server
cargo clippy --locked --offline -p fagent-node-memory -p fagent-scene-input -p fagent-storage -p fagent-server --lib -- -D warnings
cargo test --locked --offline -p fagent-scene-input --lib i5_ -- --test-threads=1
cargo test --locked --offline -p fagent-storage --lib i5_ -- --test-threads=1
cargo test --locked --offline -p fagent-server --lib i5_ -- --test-threads=1
cargo test --locked --offline -p fagent-server --lib all_match_criteria_scopes_paths_and_unicode_pages_match_full_reference -- --test-threads=1
cargo test --locked --offline -p fagent-server --lib scoped_tree_full_bytes_and_cursors_cover_dependencies_and_metadata -- --test-threads=1
cargo test --locked --offline -p fagent-server --lib committed_fixture_reads_from_memory_match_today -- --test-threads=1
cargo test --locked --offline -p fagent-storage --test node_memory_writes sqlite_i4_delta_smoke_and_db_failure_rollback -- --test-threads=1
cargo fmt -p fagent-node-memory -p fagent-scene-input -p fagent-storage -p fagent-server
cargo fmt --check -p fagent-node-memory -p fagent-scene-input -p fagent-storage -p fagent-server
git diff --check
```

Final check/lint/fmt/diff passed. Focused I5 smokes: scene-input 1, storage 3, server 2. Additional existing small-fixture smokes: direct frozen find 1, direct frozen context/metadata/cursors 1, HOT/off committed fixture 1 and SQLite strict/page/SQL-failure rollback 1, all passed. The committed-fixture differential ran before final cache transition cleanup; final storage cache/rollback oracles cover that cleanup. Initial fmt --check reported new code formatting; cargo fmt corrected it. No full suite or generated fixture was run.

At `fa842f4`, all 44 modified/new Rust files stayed below 500 lines; the largest was existing `storage/src/revision_indexes.rs` (290 lines), still responsible for legacy revision metadata/cache compatibility. New modules separate typed access, capability ordering/navigation, read-only dependency closure and authored output formatting by responsibility. No >1000-line reason was needed.

## Coordinator check at the partial checkpoint

After the worker handed off `30b0690`, the coordinator independently reran locked/offline
storage/server cargo check, package fmt check, the direct frozen authored byte/cursor
smoke (1/1) and `git diff --check 78cfdb0`; all passed. No broader tests or measurements
were run. The user then changed the Codex delegation rule to separate subsessions;
AGENTS recorded that rule, with Claude Code unchanged. That policy update alone did not
authorize either patch; I5 remained partial until the user's later specific approval.

## Completion checks in the new subsession

From baseline `2439fc4`, using the same target directory and two build jobs, sequentially:

```sh
cargo check --locked --offline -p fagent-storage -p fagent-server
cargo test --locked --offline -p fagent-storage --lib i5_ -- --test-threads=1
cargo test --locked --offline -p fagent-storage --test node_memory_writes i5_ -- --test-threads=1
cargo test --locked --offline -p fagent-server --lib i5_ -- --test-threads=1
cargo test --locked --offline -p fagent-scene-input --lib i5_ -- --test-threads=1
cargo clippy --locked --offline -p fagent-node-memory -p fagent-scene-input -p fagent-storage -p fagent-server --lib -- -D warnings
cargo fmt -p fagent-storage -p fagent-server
cargo fmt --check -p fagent-node-memory -p fagent-scene-input -p fagent-storage -p fagent-server
git diff --check
```

Focused tests passed: storage lib 5, SQLite integration 1, server lib 3, scene-input lib 1. New checks compare legacy guard errors for both dependency endpoints, unrelated success, invalid pages/source-destination, missing/gapped/oversized/versioned/tampered/absent coverage; edit-context bytes including >100 dependency summary; actual HOT stale strict dispatch, dependency conflict, replay, SQL-failure rollback, retained old reader and recovery; full snapshot/shared_snapshot completeness; HOT scene frame/error/order selection, prerender selection and font metadata. Existing Prepared oracle covers full-vs-HOT input, keys, revision/resources and note overflow intent. An initial test fixture referenced a token before creation, then lacked enough distinct dependencies for truncation; these fixture mistakes were corrected before passing. A test Arc serialization error was also corrected. No product failure was observed in the bounded runs.

Final cargo check, strict library clippy, package fmt check and diff check passed. The completion diff changes 13 Rust files; the largest is `storage/src/shadow/incremental.rs` (281 lines), which keeps one cohesive staged-batch/strict-guard responsibility. Two new test modules isolate the legacy guard oracle (173 lines) and actual SQLite dispatch/rollback smoke (99 lines). All touched Rust files are below 500 lines; no >1000-line exception is needed. Unchanged AGENTS is 72 lines / 9,904 bytes.

Full HTTP snapshot, export and bundle entrypoints were not changed by either artifact; completeness was exercised at their storage snapshot/shared_snapshot boundary, not through an HTTP/export/bundle E2E run. No listener was started.

## Coordinator acceptance after subsession handoff

The subsession `01a1e39e-3f4f-7d02-a087-e61e31f47358` completed at product commit
`12bdbd3` with a clean tree. Only after that handoff, the coordinator independently
ran storage/server locked/offline cargo check, four-package fmt check, the targeted
`i5_strict_history_and_both_dependency_endpoints_match_legacy` smoke (1/1), and
`git diff --check 2439fc4..HEAD`; all passed. No parallel source work or broader tests
were performed while this subsession ran. The two previously blocked artifacts are
now applied under the user's concrete approval; no approval blocker remains for I5.

## Remaining global costs and later verification

Normal successful MCP mutations now prepare follow-up prerender from the reader capability. MCP get_edit_context and supported stale strict-page writes use persistent dependencies instead of full legacy guard/index reconstruction. Unsupported/fallback operations and explicitly full APIs still materialize full state when required.

Cold loads and I4 admission/fallback validation remain full work. Context/virtual reads still select/sort their authored source closure and produce expanded/resolved nodes, resource/theme maps and output-specific indexes. Exact-total find still scans all requested nodes; large requested scopes/masters remain large work. Resource JSON vectors/header copying and the I3b 256-shard copy-on-write tradeoff remain. There is no packed/interner representation or measured RSS/latency improvement.

The bounded strict-page/history/summary/rollback and consumer oracles above now pass. Keep the later deep validation phase separate, as the user requested:

```sh
CARGO_BUILD_JOBS=2 cargo test -p fagent-node-memory -p fagent-scene-input -- --test-threads=1
CARGO_BUILD_JOBS=2 cargo test -p fagent-storage -p fagent-server -- --test-threads=1
FAGENT_NODE_MEMORY_WRITES=strict FAGENT_NODE_MEMORY_READS=strict FAGENT_NODE_MEMORY_SHADOW=strict CARGO_BUILD_JOBS=2 cargo test -p fagent-storage -p fagent-server -- --test-threads=1
bash scripts/task19-check.sh
CARGO_BUILD_JOBS=2 cargo test -p fagent-storage --test node_memory_writes postgres_i4_delta_smoke -- --ignored --test-threads=1
CARGO_BUILD_JOBS=2 cargo test --release -p fagent-server --lib generated_5k_reads_from_memory_match_today -- --ignored --nocapture --test-threads=1
WRITES_REALDATA_URLS='<disposable-off>|<disposable-on>' CARGO_BUILD_JOBS=2 cargo test --release -p fagent-storage --test node_memory_writes_realdata -- --ignored --nocapture --test-threads=1
```

All PostgreSQL/restore commands require isolated disposable databases, never the native database/port. Broader differential remains open across cache ownership races, external mutation/import/restore, all themes/component/navigation/resource combinations, histories/concurrent writers and both backends. Thirty-sample latency and retained/peak allocator or RSS measurements need the later harness work described in I3b/I4; none was run here.
