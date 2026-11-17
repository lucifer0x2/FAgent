# Verification of the base branch `feat/node-memory-writes` before starting Vello — 2026-11-16

Source: branch `feat/node-memory-writes` @`9e17583`, 49 commits ahead of `main`,
merges cleanly into `main` (`git merge-tree`), does not touch `web/`, `renderer/`, `crates/scene-wasm|layout|text`.
Run on a MacBook Pro (rustc 1.97.0). This is the "deep validation" pass that Task28 I5 still deferred
([TASK28-I5-REPORT](TASK28-I5-REPORT.md), section "Remaining global costs and later verification").

## Conclusion

**Fine to use as a base.** By default all three node-memory flags are `off`, so production behavior is unchanged;
there are no product bugs in the default mode. The points that appear with the flags on (section "Findings") and the findings of the independent
code review were fixed, except one performance item; see section "Code review and fixes".

## Results

| Check | Result |
| --- | --- |
| `cargo fmt --check` + `clippy -D warnings` (node-memory, scene-input, storage, server) | pass |
| `cargo test` node-memory + scene-input | 92 pass, 1 ignored |
| `cargo test --no-fail-fast` storage + server, default flags | 599 pass, 1 environment failure, 53 ignored (170 suites) |
| Same set, `WRITES/READS/SHADOW=strict` | 593 pass, 7 failures (see below) |
| layout, text, render, scene-wasm | 139 pass, 5 ignored |
| `generated_5k_reads_from_memory_match_today` (release) | pass |
| Temporary PG 16: postgres_contract/domain/transactions/resources/cache/namespace/fidelity | 17/17 |
| PG: server `postgres_backend`, `project_privacy`, `viewer_auth` | 2/2, 2/2 |
| PG: `node_memory_writes` (i4 delta smoke, memory writes = today, racing writers = sequential replay) | 3/3 |
| PG: `memory_diff::pg_large` (release) | pass |
| **Real data** `node_memory_writes_realdata` off↔on (release) | pass, 0 mismatch |
| **Real data** `node_memory_shadow_realdata` (release) | pass, 0 error; 7 projects, 10–7,034 nodes |

Real data: the latest native backup (SHA256SUMS match) was only **copied** to the verification machine,
and restored with `pg_restore --no-owner --no-acl` into a temporary PG 16 cluster (Homebrew, loopback, random port) on the verification machine; the cluster
was stopped and deleted. No connection to the live database, nothing changed on the source machine, no data entered the repo.

## Findings

1. **Fixed — storage tests did not compile when running the whole crate.** `crates/storage/tests/node_memory_i5.rs` is a submodule
   of `node_memory_writes.rs` (`mod node_memory_i5;`) but sat directly in `tests/`, so Cargo built it as its own test
   target and it failed (`use super::*`). Earlier checks only ran `--test node_memory_writes`, so it was not exposed.
   Fix: moved to `tests/node_memory_i5/mod.rs` like `node_memory_i4/`; the i5 tests still pass.
2. **Shadow diverges on asset readiness when a file is deleted out of band** (only when the flags are on). Three tests
   (`multi_resources`, `native_compute`, `render` — scenario: delete the image file then commit) panic in strict:
   `crates/storage/src/shadow/mirror.rs:150`, `outcome: mismatch`, the id is an asset. The `full` comparison
   (`shadow/compare.rs:110`) recomputes readiness from disk at commit time, while RAM keeps the readiness from load time. In mode `on`
   all three tests **pass** (correct behavior) and only log the divergence. This is exactly the "external mutation" area I5 left open; a decision is needed:
   refresh readiness in RAM on commit, or drop readiness from the shadow comparison.
3. **The `RevisionIndexes` cache expectation in tests no longer holds when HOT reads are on.** `revision_indexes` (lib + test) and
   `scoped_reads` assert that two `shared_snapshot` calls share the same indexes `Arc`; when reads are on, `scoped_snapshot`
   calls `claim_cache_cleanup` and evicts this cache (`backend/sqlite.rs:107-124`, as I5 intends), so it fails in both `on`
   and `strict`. These tests need a flag condition; also measure whether full-snapshot callers on a HOT project
   rebuild the indexes each time (not yet measured).
4. **Environment, not a bug:** `native_layout_parity` needs a pre-captured Inter font fixture
   (`spikes/native-render-parity/out/fonts/inter.ttf`, the test deliberately does not skip); `postgres_cache_failures` needs its own
   Docker container — not run yet. Render tests need the built renderer (`npm run build:renderer`) + Playwright Chromium
   (`PLAYWRIGHT_BROWSERS_PATH`).

Points 2 and 3 were fixed afterwards (section below).

## Code review and fixes

A subagent reviewed the whole branch: no correctness bugs with the flags `off`. With the flags on: the next PG writer takes the slow path because the
transaction commits before the batch publishes HOT; a read `adopt` replaces a newer handle; TTL/budget do not run when HOT;
`editGuard` does not say whether the guard is enforced. All were fixed together with points 1–3 above; still deferred: loss of the validation certificate
after a fallback commit (performance only). Details and evidence: [TASK28-REVIEW-FIXES](TASK28-REVIEW-FIXES.md), section
"Follow-up review". After the fixes: default 691 pass, strict 599 pass (previously 7 failures), PG 24/24 + server 2/2 in both default
and strict, real data pass. `postgres_write_through` (outside the gate) fails identically on the original `9e17583` — an old test, not fixed.

## Not done

30-sample latency/RAM benchmark, restore matrix, full differential by theme/component/race (outside the scope of this check,
as Task28 recorded).

## Re-run

```bash
cargo test --locked --no-fail-fast -p fagent-storage -p fagent-server -- --test-threads=2
```

PG: create a temporary PG 16 cluster, `TASK19_POSTGRES_URL='postgres://postgres@127.0.0.1:<port>/task19'` (URI; the i4 test splits on
`/`), commands in `scripts/task19-check.sh` and TASK28-I5-REPORT; real data needs `WRITES_REALDATA_URLS` and
`SHADOW_REALDATA_URL` pointing at a single-use restored DB.
