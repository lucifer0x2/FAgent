# Task28 — two P2 review fixes, 2026-11-16

Both findings were confirmed and fixed locally in `FAgent-node-memory`, branch
`feat/node-memory-writes`. The authorized scope remains bounded regression checks;
this does not close Task28's deferred deep verification or performance work.

- Baseline source: `95895633093e7cb980718a89f1d0c0bdfa52cf89` (clean).
- Code/tests commit: `0f62576cd4b342f87a1ad6457ca5a91080a19551`.
- Code-commit tree: `8efd5b345da5a2ca149a3e10903380a928bc8449`.
- Source of findings: the user's transferred external review, at `batch.rs:134`
  and `concurrent/writes.rs:42` on the baseline. These line numbers identify the
  reported baseline, not the updated source.

## Findings and changes

**Malformed pageIds reached durable history in nonstrict mode.** Shape validation
was inside the strict semantic guard, while bundle replay independently rejected
empty/duplicate/oversized/invalid-ID page lists. `scoped::request_value` now checks
shape before hashing, replay, HOT staging or fallback persistence in both SQLite
and PostgreSQL. `PageIndex::validate_page_ids` includes the existing API/bundle ID
grammar; restore validation and the HOT strict guard reuse it. Project/key and
operation-count checks still precede this check. Valid request hashes/replays are
unchanged; malformed requests fail with `VALIDATION_FAILED`, field `pageIds`,
before reserving any key or advancing revision/history. Valid nonstrict pages
still impose no existence, affected-page, dependency or stale-base guard.

Inline, preview and scoped paths share this boundary. Storage's file request has
no page list; MCP file handling reads operations and converges into the same scoped
API. Its existing end-to-end smoke was run below. No migration, existing-history
repair, automatic resealing or history-format change was added.

**Asset import discarded known concurrent pairs.** Both backends actually store
`[{op:"import_asset",asset:<Asset>,font:<null|FontRegistration>}]` inside the existing
scope envelope. Bundle replay retains this operation array. The concurrent parser
had no matching opcode, while an old unit fixture misleadingly supplied an
arbitrary object as a resource no-op. It now recognizes a singleton import only
after deserializing the asset and optional font metadata, with exactly the three
stored operation fields. Imports contribute no node/key writes and therefore no
spurious pairs. Other writes keep their latest revisions, total and 20-pair limit.
Unknown opcodes, non-array objects, malformed imports, mixed import arrays,
missing/gapped/capped history remain conservative `concurrentUnknown`. Imports'
exact-revision rule and the user batch operation API were not changed.

## Executed evidence

All Cargo commands used the existing shared target and two build jobs, sequentially:

```sh
export CARGO_TARGET_DIR=target
export CARGO_BUILD_JOBS=2
cargo test --locked --offline -p fagent-storage --test task28_review_fixes -- --test-threads=1
cargo test --locked --offline -p fagent-storage --test task28_review_fixes malformed_page_ids -- --test-threads=1
cargo test --locked --offline -p fagent-storage --lib concurrent::tests -- --test-threads=1
cargo test --locked --offline -p fagent-storage --lib i5_ -- --test-threads=1
cargo test --locked --offline -p fagent-storage --test task28_review_fixes --test scoped_concurrency --test scoped_bundle --test batch_file -- --test-threads=1
cargo check --locked --offline -p fagent-storage -p fagent-server
cargo clippy --locked --offline -p fagent-storage --lib -- -D warnings
cargo test --locked --offline -p fagent-server --test scoped_concurrency sqlite_scoped_file_preview_replay_and_original_schema_validation -- --test-threads=1
cargo fmt -p fagent-storage
cargo fmt --check -p fagent-storage
git diff --check
git diff --cached --check
```

| Check | Observed result |
| --- | --- |
| Baseline regressions, before product edits | 0/2 passed: empty page list preview returned success; stale write returned `concurrentUnknown:true`. Reordered first assertion and reran the page test: empty list commit also returned success at revision 3. |
| New integration regressions, final | 2/2 passed, flags explicitly off and WRITES/SHADOW strict + READS on; HOT dispatch counter advanced. Invalid shape commit/preview leaves revision, node rows, commit/idempotency counts unchanged. A corrected request reuses the rejected key. 100 valid nonexistent IDs retain stale LWW; resource fallback accepts valid nonstrict pages; bundle restore and replay succeed. Actual image and font imports preserve latest conflicts before/after restore. |
| Concurrent unit checks | 6/6 passed: raw/enveloped import, optional font, latest revisions, 25 total / 20 reported pairs; unknown/malformed history, gaps, null/size cutoff and 1001-commit cap. |
| Existing I5 library checks | 5/5 passed, including strict guard/reference parity and history failure cases. |
| Existing targeted storage checks | File 4/4, scoped bundle 1/1, SQLite scoped concurrency 3/3; PostgreSQL scoped contract intentionally ignored. |
| Compile / lint / formatting / diff | All passed; storage compile includes PostgreSQL dispatch. |
| Existing real MCP file smoke | 1/1 passed after normal escalation for a disposable random loopback listener; the first sandboxed run failed with bind `EPERM` before exercising the request. |

The integration test initially referenced a nonexistent `StorageError.details`
field; corrected the fixture to `field` before recording the baseline red runs.
Formatting the sibling worktree also needed normal sandbox escalation and then
succeeded. No automatic approval rejection or alternate execution route occurred.

## Scope, module sizes and remaining work

Code/tests diff: seven Rust files, +326/-21. Production edits stay in existing
request-shape, strict-guard, bundle-replay and concurrent-parser responsibilities;
the new integration file owns only these two regressions. Sizes: scoped history
222, scoped index 139, bundle history 183, concurrent writes 90, concurrent tests
208, HOT incremental 273 and new integration test 225 lines. All are below 500;
no split or >1000-line exception is needed. Unchanged AGENTS: 72 lines / 9,904 bytes.

No full suite, generated 5k/200k workload, PostgreSQL service/runtime/restore matrix,
latency/RAM benchmark, N7 packing or unrelated I5 optimization was run or added.
PostgreSQL import envelope equivalence is source-backed and compile-checked, not
runtime-tested here. No push, deployment, restart, live port/native database,
private data or `~/.fagent` access; fixtures used disposable temporary roots.

## Main-session acceptance

After subsession handoff at `4ce6b05` (code/tests `0f62576`), the main session
reviewed the production diff and independently reran the two integration
regressions: 2/2 passed. Storage/server `cargo check --locked --offline`, storage
`cargo fmt --check` and `git diff --check 9589563..HEAD` also passed. No additional
product edits, deep tests, PostgreSQL runtime checks, benchmarks or push occurred.

## Follow-up review of `9e17583` — 2026-11-16

An independent code review of the whole branch (before the Vello work)
found no correctness issue with the three node-memory flags `off` (the default). With
flags `on`/`strict` it found the following; all fixed except 2.

1. **The next PostgreSQL writer missed HOT.** The transaction commits (releasing the
   project row lock) before the staged batch publishes, so the next writer or reader
   saw the previous revision and took the full path or reloaded.
   `shadow::reads::published_revision` waits once on the state lock when memory is
   behind the probed revision; `serve` and `memory_handle` use it.
2. **Not fixed (performance only, flags on):** after a fallback commit the validation
   certificate is lost, so the next HOT write revalidates. Fixing it needs the full
   path's instance expansion result threaded through; left as a follow-up.
3. **A read could replace a newer handle.** `adopt` now keeps a registry handle of the
   same schema and incarnation whose published revision is already past the read's.
4. **TTL and budget never applied under HOT traffic.** Read hits and writes call
   `Mirror::maybe_expire`, which runs `expire` at most once a minute.
5. **`editGuard` did not say whether page guards were enforced.** The scoped HTTP
   response now carries `strict`; documented in `docs/CONTRACT.md`.

Known issues from the deep verification, fixed in the same change:

- (a) `tests/node_memory_i5.rs` was built as its own test target and failed to compile;
  moved to `tests/node_memory_i5/mod.rs`.
- (b) Strict shadow reported a mismatch when an asset file was removed out of band:
  readiness is computed from disk at commit time. `Mirror::find` now reloads a project
  whose asset/font readiness drifted (`resyncs` counter) instead of comparing it.
- (c) Legacy snapshot / `RevisionIndexes` cache tests assumed HOT reads were off; they
  now pin the flags off (`scoped_support::legacy_store`, `postgres_support::cached_legacy`).

### Evidence (MacBook Pro, temporary PostgreSQL 16 cluster, ICU collation)

| Suite | Result |
| --- | --- |
| `fmt --check`, `clippy --lib -D warnings` (storage, server) | pass |
| node-memory, scene-input, storage, server; flags default | 691 pass, 1 environment failure, 54 ignored |
| storage, server; WRITES/READS/SHADOW `strict` | 599 pass, 1 environment failure (before: 7 failures) |
| PG storage suites (contract, domain, transactions, resources, cache, namespace, fidelity, privacy, auth, node_memory_writes, revision_indexes); default and strict | 24/24 each |
| PG server `postgres_backend`; default and strict | 2/2 each |
| Real data (native backup copy) `node_memory_writes_realdata`, `node_memory_shadow_realdata` (release) | pass, 0 mismatch |

The environment failure is `native_layout_parity`, which needs the captured Inter
fixture. `postgres_cache_failures` needs Docker and was not run.
`postgres_write_through` (not in the PG gate) fails identically on unmodified `9e17583`
in both modes: a stale conflict now returns `VALIDATION_FAILED`, and with flags on its
cache expectations predate HOT reads. Pre-existing `clippy --tests` errors
(`scale_tests.rs:30`, `node_memory_shadow_bench.rs:162`) are outside the `--lib` gate.
