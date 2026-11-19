# PostgreSQL runtime adapter

`PostgresBackend` implements the same blocking `StorageBackend` contract as
SQLite, including complete product validation, notes, tokens, styles, instances,
themes, resources, dry runs, history and exact idempotency replay.

Set `DATABASE_PROVIDER=postgresql` and supply a nonempty `DATABASE_URL` in the
environment before launching the server. The database role needs permission to
initialize and use the runtime schema. The server connects during startup
and exits on initialization failure. Hot projects live in node memory, as on
SQLite.
PostgreSQL mode stores nothing in the data root; it only reads `import_asset` /
`import_assets` `path` files and `apply_batch_file` files from
`<data-root>/<slug>/inbox` (default `~/.fagent`) and creates no directory there.

For an embedded server, inject the adapter at the composition root:

```rust,ignore
use fagent_storage::{PostgresBackend, Storage};

let backend = PostgresBackend::connect(&database_url)?;
let storage = Storage::new(backend);
let state = fagent_server::AppState::new(storage, server_config);
let app = fagent_server::router(state);
```

The backend owns its node memory; there is no cache to construct or inject.
`backend.database_namespace()` returns the durable SQL namespace that every
identity carries.

## Connections and timeouts

`PostgresBackend::with_config(PostgresConfig::new(database_url))` accepts these
public configuration fields:

| Field | Default | Meaning |
| --- | --- | --- |
| `pool_size` | 4 | Dedicated reusable synchronous client workers; allowed 1–32 |
| `connect_timeout` | 3 seconds | Driver socket connection attempt limit |
| `statement_timeout` | 15 seconds | PostgreSQL statement limit |
| `lock_timeout` | 3 seconds | PostgreSQL lock acquisition limit |
| `operation_timeout` | 30 seconds | Total caller checkout and result wait limit |
| `schema` | `fagent_runtime_v1` | `DatabaseSchema` holding the runtime tables; first on every session's `search_path` |

All methods block and belong on the runtime's existing `spawn_blocking` boundary.
Workers own the synchronous driver's reactor; no reactor is entered on a caller
thread. There is no mutex serializing independent project reads or writes.
Connections are reused and closed connections are discarded. Network keepalive
and TCP user timeout are also configured; operating-system support varies.

The caller wait limit also covers a stalled authentication exchange or broken
network after a statement was sent. A stalled worker retains its bounded pool
slot until transport recovery; additional workers are not spawned. A timed-out
write may have committed: retry its identical idempotency key and body to recover
the original result. No successful acknowledgment precedes durable SQL COMMIT.

The initial transport uses `NoTls`. Use it for a trusted local connection or an
explicitly secured local tunnel/proxy. `sslmode=require` is unsupported and fails
without exposing the connection string. Native TLS configuration is a later
deployment concern, not an implicit downgrade of a required TLS connection.

## Schema and persistence

Construction transactionally initializes version 1 of the runtime schema and its
durable random database namespace. The schema is `fagent_runtime_v1` unless
`PostgresConfig::schema` (the server: `FAGENT_DATABASE_SCHEMA`) selects another, for
example `public`. `DatabaseSchema::parse` accepts 1–63 lowercase ASCII letters, digits or
`_`, starting with a letter or `_`, and rejects `pg_*` and `information_schema`, so the
name is safe to quote into SQL; every session puts it first on `search_path`, ahead of
`pg_catalog`. An existing schema such as `public` is used as is (no `CREATE SCHEMA`, so
no database CREATE privilege is needed); a missing one is created. An unsupported schema
registry version fails before product table initialization.
Construction creates no projects or demo data and touches no local data root.

Projects have monotonic identity generations. Mutations acquire the project row
`FOR UPDATE`, then run a separate replay query before checking the expected
revision. The read-committed replay query sees a writer that released that lock.
All changed entity rows, revision, whole commit record, canonical request hash,
original response and any asset bytes commit together with
`synchronous_commit=on`. Entity upserts/deletions use set-based arrays and persist
only entities whose serialized domain value changed.

Snapshots and resource/font reads use repeatable-read transactions. Entity
payloads and history/replay values are opaque escaped JSON in SQL `TEXT`;
project names are UTF-8 `BYTEA`. This preserves existing NUL and JSON-number
semantics that PostgreSQL `JSONB`/literal text would otherwise reject. Assets are
project-scoped `BYTEA`, verified against their manifests; oversized resource
payloads are rejected before download. The pinned bundled font is seeded only
by explicit project creation. `apply_batch_file` and `import_asset` paths use the
local inbox above (`Storage::with_inbox_root`); an embedded `Storage` without an
inbox root returns `STAGING_UNSUPPORTED` for both.

A batch is staged in node memory under the `FOR UPDATE` project row lock and
validated on its delta (the whole project is validated once per load). Every
latest-revision read from memory requires a successful PostgreSQL identity
probe that matches the resident copy; a stale copy is evicted and reloaded.
Rotating the SQL namespace changes every identity, so resident copies are
reloaded. Identities carry no asset readiness: a binary deleted directly in the
database stays "ready" for a resident project until it is reloaded. See
[snapshot caches](MOKA-SNAPSHOT-CACHE.md).

SQLite migration, production cutover/rollback, backup/restore administration,
retention pruning and Kubernetes are separate work. A
database restore must rotate its database namespace if it reuses prior project
generation/revision values while preserving different authored content.
