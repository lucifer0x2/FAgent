# Performance

Benchmarks recorded in the repository for the backend, node-memory and the viewer.

Every number below comes from a benchmark recorded in the repository; none was rerun for this
page. They are single runs on one machine, so read them as orders of magnitude, not guarantees.

**Backend, large project.** The [Task24 harness](TASK24-REPORT.md) copies a real
6,711-node project 1× and 30× (30× = 201,330 authored / 599,400 expanded nodes, 570 pages) and
times MCP reads and a one-text-node commit. It ran on an Apple M5 (10 cores, 24 GiB) with a
release build. PostgreSQL 16 ran in a container limited to 2 CPUs / 2 GiB. Values are
**median / p95 ms**. "Before" is the original baseline; "now" is the rerun after the incremental
validation and page-scoped reads of [Task25](TASK25-REPORT.md#task24-rerun-after-slice-4--2026-11-13).

| Operation | Project | SQLite before → now | PostgreSQL before → now |
| --- | --- | --- | --- |
| `get_design_context`, one frame | 1× (6.7k nodes) | 788.8 / 876.2 → **11.3 / 11.5** | 765 / 787.2 → **3.8 / 4.0** |
| `get_design_context`, one frame | 30× (201k nodes) | 42,608.6 / 46,357.8 → **212.4 / 216.4** | 30,426.1 / 34,675.8 → **403.1 / 410.2** |
| `get_outline`, one page | 30× | 1,824 / 2,529.8 → **249.2 / 268.3** | 1,233 / 1,280.8 → **417.8 / 467.8** |
| Commit, strict (`apply_batch`) | 1× | 404.8 / 422 → **63.5 / 65.7** | 377.2 / 389.8 → **98.2 / 129** |
| Commit, strict | 30× | 12,723.7 / 13,348.2 → **2,030.6 / 3,547.4** | 12,639 / 13,294.1 → **2,444.4 / 2,789.7** |
| Commit, page-scoped (`pageIds`) | 30× | 38,871.9 / 40,419.7 → **1,880.9 / 3,403** | 34,121.9 / 55,458.4 → **2,269.4 / 2,646.1** |
| Storage cold snapshot load | 30× | 225.5 / 256.8 | 356.2 / 421.9 |

The 30× responses stay the same size as at 1× (outline 72 KB, context 92 KB): the work grows
with the project, but the answer does not. Write latency at 30× is still above the ~1.5 s target
recorded in the report; validation dominates it.

**Node-memory (always on).** [Task28](../tasks/28-node-memory-integration.md) introduced it and
[Task41](../tasks/41-node-memory-engine.md) phase 1 made it the only path: hot projects stay in RAM
(`crates/node-memory`, struct `NodeMemory`) with indexes updated per operation, every latest-revision
read and every batch goes through it, and the database stays the source of truth. There are no
flags or modes any more; the "Flag off" and "Flag on" columns below are the Task28 measurements of
the former database-only path and the opt-in memory path. The figures below were measured on a
generated 200k-node fixture with PostgreSQL and a release build; the source does not record the
hardware. This fixture differs from the Task24 project, so do not compare it with the table
above.

| Measurement (PostgreSQL, 200k-node fixture) | Flag off | Flag on |
| --- | --- | --- |
| `get_design_context`, one frame (`READS`) | 156 ms cached; 207 ms right after a write | **1.5 ms** warm; **2.0 ms** right after a write |
| Page-scoped commit, median of 30 (`WRITES`) | 636 ms (load 200, persist 34) | **468 ms** (load 29, persist 3) |
| Same commit, real project of 7,025 nodes | 28.5 ms | **22.5 ms** |
| Shadow mirror cost per commit (`SHADOW`) | – | 114 µs (17–26 µs on real projects); first load 76 ms |

Task41 phase 1 (always on, same 200k fixture: 178,604 nodes, PostgreSQL, release, median of 30, run
back to back on an idle machine): a small props edit commits in **36.5 ms** (`memory.begin` 30.4,
persist 4.8), against 55.9 ms for the former opt-in memory path and 988 ms for the former default
path in the same session. The first commit on a cold project loads and admits it under the row lock
(about 3.8 s at 200k). Ops the delta validator does not cover yet (pages, root ops, tokens, styles,
notes, `theme`/`links`/`flowStart`/`animations`) still validate the whole project, in memory.
See [TASK41-REPORT.md](TASK41-REPORT.md).

In-memory operations on the 200k fixture take well under a millisecond: a props update takes
584 ns, and a begin + commit takes 542 ns. Memory use is about 1.3 KB of RAM per authored node.
Later node-memory stages (persistent reader indexes, delta validation, HOT reader cleanup) have
been implemented, but their benchmarks are deferred. See the
[I3b](TASK28-I3B-REPORT.md), [I4](TASK28-I4-REPORT.md) and
[I5](TASK28-I5-REPORT.md) reports and the
[base verification](FAGENT-BASE-VERIFICATION.md).

**Viewer.** These numbers come from an Apple M3 Pro with a production build
([details](VIEWER.md#measurements--2026-11-17)). On a board of 12 frames and 4,945 nodes,
the Vello engine reaches "Scene rN" in 1.8–1.9 s, against 2.8–3.6 s for Canvas2D in the same run.
Panning at Fit draws in 13.3 / 13.8 ms (p50 / p95). No main-thread task exceeded 50 ms while
panning or zooming.
