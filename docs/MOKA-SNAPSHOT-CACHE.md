# Snapshot caches

The only snapshot cache left in the runtime is the server's **HTTP snapshot response cache**
(Moka 0.12.15, `crates/server/src/snapshot_http.rs`). Latest-revision snapshots themselves come
from node memory (`NodeMemory`, always on; see [Task 41](../tasks/41-node-memory-engine.md) and
[TASK41-REPORT](TASK41-REPORT.md) "Phase 4"): `snapshot()` and `snapshot_with_cache_identity()`
return a copy of the resident project when it holds the revision the database reports, and
otherwise read the database and load the project into memory. `database_snapshot()` always reads
the database.

## HTTP snapshot response cache

The snapshot route keeps an in-process Moka cache in server state. It stores immutable
serialized bytes and a strong ETag per source identity (`SnapshotCacheIdentity`), node format and
visibility. The loopback and public viewer routes share this cache. Every request revalidates
against storage (an identity probe) before serving bytes or returning 304. Only snapshot reads
populate entries. A read concurrent with a commit can observe either revision at its
authoritative read point. On a miss the route probes the identity, then reads the snapshot with
the identity of that read (a second probe inside the memory-first read).

PostgreSQL probes the canonical project ID, database namespace, project generation, schema and
revision; its identity carries no asset readiness. Administrative PostgreSQL recovery to a
different revision history must rotate the database namespace before serving requests. The
SQLite backend probes the canonical project, schema and revision from the live database; on Unix
it also checks the database file identity and current asset readiness, which can change
independently of revision. SQLite storage expects a single owner of the data root and offline
administrative database edits. Backends without a reliable identity use a fresh snapshot read
for each request.

A miss binds encoded bytes to the identity of the snapshot read. A late fill at an older revision
uses a separate key. Entries expire 1 hour after admission, with a 64 MiB weighted capacity and a
16 MiB admission limit per entry. The weight includes the body, tag, identity and a fixed overhead
allowance. Older entries can remain until eviction or expiration. Oversized responses still serve
normally.

```sh
cargo test --offline -p fagent-server --test snapshot_http
bash scripts/task19-check.sh   # includes server postgres_backend (ETag across generations and namespaces)
```

## History

- **PostgreSQL snapshot cache** (Task 19, 2026-11-03, replacing the Redis/bitcode cache of
  [REDIS-BINARY-CACHE](REDIS-BINARY-CACHE.md)): one typed snapshot per project, TTL 1 hour,
  64 MiB, write-through after commits, every hit confirmed by a PostgreSQL identity probe.
  Removed in Task 41 phase 4 (2026-11-19) together with `SnapshotCache`, `with_cache`,
  `cache_namespace` (now `database_namespace`) and the `pgmoka` benchmark engine.
- **Revision metadata cache** (`IndexCache`, Task 25): `RevisionIndexes` (topology, pages,
  references, lazy dependency and read-scope indexes) cached per `SnapshotCacheIdentity` for
  commit validation, page guards and `get_edit_context`. Removed in Task 41 phase 4: commits stage
  in node memory, and `RevisionIndexes` is now built once per database read (`shared_database`,
  the fallback of scoped reads and `get_edit_context`).
- Measurements of both caches stay in [RUNTIME-STORAGE-BENCHMARK](RUNTIME-STORAGE-BENCHMARK.md)
  (run C) and [TASK25-REPORT](TASK25-REPORT.md) as history. Rejected-approach entry:
  [REJECTED-APPROACHES](REJECTED-APPROACHES.md).
