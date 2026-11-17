# FAgent — v1 implementation plan

Implementation status 2026-10-28: M0–M2 have been accepted with fixture/module tests
and real local E2E; M3 resolution/context/layout and M4 storage hardening have been
verified within the published scope. Evidence and limits are in `docs/ACCEPTANCE.md`;
run instructions are in `README.md`. The AC below remain the source of requirements.
The workspace now has Git and implementation code; changes are not yet committed/pushed.
Goal: an agent creates and reads designs through MCP; the user sees the result in a local browser.
The current product priority is displaying designs, not building a mouse-driven editing tool.

## 1. Product constraints and implementation choices

Agreed with the user:
- The Rust backend holds the design model, revisions, change validation and project state.
- The agent talks to the user in Codex/Claude outside FAgent.
- The agent reads/writes nodes, styles, layout, tokens and asset metadata through MCP in the MVP itself.
- Storage uses SQLite; each project is a directory `~/.fagent/<project>/` containing `db.sql` and `assets/` for every asset/font of the project.
- On 2026-11-02 the user chose PostgreSQL for the multi-backend/continuous-redeploy direction and requested Task15. The isolated spike came first; the current SQLite runtime and real data are not migrated. Choosing the engine does not mean the product port or Kubernetes HA is complete.
- On 2026-11-02 the user requested a common storage trait so the engine can be swapped. Task16 moves the runtime behind `StorageBackend` and a SQLite adapter, keeping the API/model/transaction invariants; no production PostgreSQL or migration yet.
- On 2026-11-02 the user approved Task17: PostgreSQL commits the delta before ACK, and Redis serves HOT reads in the Task15 spike; no change to the runtime or real data. Redis-primary/write-behind is kept as Task18 for later research.
- On 2026-11-03 the user approved Task19: implement a full PostgresBackend through the product's StorageBackend and a Redis HOT snapshot if appropriate; the binary format is **bitcode instead of zero-copy**, one latest snapshot key per project. Reuse every validator, durable revision/history/idempotency and resource bytes. No engine-selection CLI or real-data migration/cutover yet.
- After the Task19b benchmark and a review of Redis 8 JSON, the user decided to replace the runtime Redis/bitcode cache with **Moka in-process, TTL 1 hour**. Keep the typed snapshot, one latest entry per project, PostgreSQL confirming identity/revision, and the existing HTTP/MCP contract. The two Redis/bitcode rounds are the baseline.
- Try row-oriented JSON by schema, opt-in, for agents and the HTTP snapshot. GET snapshot uses Moka for already-encoded bytes and returns the cache only when storage confirms the current version. Conditional GET must distinguish representations and return the new one after a commit. The default contract is unchanged.
- The user approved choosing storage at startup via `DATABASE_PROVIDER=sqlite|postgresql`, default SQLite. PostgreSQL requires `DATABASE_URL` and enables the Moka snapshot with a 1-hour TTL. `PORT` defaults to 3847 and can be overridden with `--port`. Choosing an engine does not move projects between the two storages automatically.
- On 2026-11-03 the user requested a local Kubernetes setup on a new branch from `feat/postgres-runtime-storage`. Task20 uses the existing OrbStack cluster, PostgreSQL with a volume and one FAgent replica; access is loopback via port-forward, no HA or real-data migration yet. AC and verification are in `tasks/20-local-kubernetes.md`.
- Following Task20, the user requested a read-only Kubernetes listener `3858` to be forwarded through Tailscale Funnel. Use the existing public viewer router and the machine's HTTPS origin; do not enable Funnel automatically.
- The user requested an optional password for the public viewer `3858`, open by default. Local MCP manages the password and sessions. Browsers sign in with an HttpOnly JWT cookie valid for 90 days. Sessions are stored in the database, bound to one IP address, and revoked when the same token appears from another IP. The contract and proxy configuration are in [docs/SERVER.md](docs/SERVER.md#public-viewer-password).
- The user requested that each project has `privacy_status: public | private`, default `private` for both old and new projects. The local port `3857` views and updates the status over HTTP/MCP. The public port `3858` only lists and serves `public` project data, even when the visitor is signed in.
- On 2026-11-03 the user requested reviewing and fixing the 10 ink warnings of U07 on Kubernetes 3857: distinguish runtime glyph-measurement deviations from real overflow/clip, keep the authored design unchanged and re-check the target runtime.
- The `db.sql` file is a SQLite binary; keep the `.sql` suffix the user chose.
- The local browser prioritizes read-only design display; no editing, chat or terminal in the product.
- Addition requested by the user on 2026-10-30: a font resource management UI for the project to add/import and download fonts; the scene stays read-only. Fonts use file bytes stored with the project or bundled fonts with pinned hashes, with no system font installation required. SVG export must carry the fonts used so it opens on another machine; the design's fonts are not changed automatically. Behavior details and evidence are in `docs/FONTS.md` after implementation.
- The user also approved all 5 priority groups of the Figma report: metrics/wrap and glyph diagnostics; tracking/alignment/decoration/case; capability and semantic-text advisories; static face selection; corners/shadow/gradient. Rich runs, variable axes, blur, responsive % and the shelved Rust engine are not opened. Adding a property must go through validator/resolution/measurement/scene/canvas/preview and supported SVG or a clear diagnostic, keeping the authored source and valid old data.
- Addition approved by the user on 2026-11-02: Task14 P1–P5 for scoped frame/instance themes, paint overrides, instance root paint, pure JSON inbox batches and dry run; MCP-first authoring with schema/fewshots/revision/idempotency. P6 contrast check stays in the backlog; the U07 design worker rebuild and golden comparison come after the API, and those image/payload AC are not yet considered met.
- The frontend uses a light tone in the Figma editor direction, without requiring a pixel-perfect clone: white panels/topbar, light gray canvas, dark high-contrast text, thin dividers and a blue accent for selection/focus.
- The UI is compact and neutral so the design stands out; layout with topbar, layers on the left, main canvas and read-only inspector on the right. Panels can be collapsed; status is shown compactly. No dark theme or UI theme toggle needed yet.

Implementation recommendations; renderer/layout must pass the M0 spike before being locked:
- SQLite WAL is decided for the MVP; benchmarks verify performance against the expected workload and determine operating limits.
- One resident backend instance serves both MCP and the viewer, using the same state and writer queue.
- MCP uses Streamable HTTP; the viewer uses HTTP to read snapshots and SSE to receive changes.
- Project assets/fonts live in `~/.fagent/<project>/assets/`; no asset binary is written into node JSON.
- Frontend React + TypeScript + Konva runs in the local browser, read-only for the design.
- The application schema is independent of Konva; the Konva tree is not stored as the original design model.
- A batch mutation must be atomic, with `expectedRevision` and an idempotency key.
- The backend commits durably before reporting success or publishing SSE.
- Basic revisions are needed from the start; visual history, diff and advanced inspect come after the display slice.
- No pixel-perfect promise across every OS, browser or font that has not been supplied.

## 2. Spike proposal and scope limits

Recommendation: one shared web renderer module for the viewer and a headless browser worker.
The worker measures text and renders PNG; Rust manages layout, scene and all application state.
Chromium and the worker's JavaScript runtime are a deliberate dependency of this approach.
A Rust backend does not mean the whole system is Rust-only or needs no browser runtime.
The first spike must verify the approach before locking the library and the worker protocol.
If the spike fails, stop expanding layout and record a concrete replacement decision before further implementation.
The read-only viewer and the data contract do not depend on whether the layout engine has been locked.

The MVP supports page, frame, group, rectangle, text and image; a group uses layout or free coordinates.
Layout includes row/column, padding (one number or 1–4 CSS-style sides), gap, `align`/`justify` (P08a), fixed/fill/hug, min/max and opt-in row wrap approved on 2026-10-28.
A root frame has a numeric width; height is numeric or `hug` to content (only with row/column layout); percentage cycles or fill on an ambiguous hug axis are not supported.
Text uses one font/style per node, plain text, explicit line height and wrapping by width.
Image only needs fit/contain/cover, simple crop, opacity and clip by frame.
The MVP has literal tokens of color/dimension/number kind, primitive styles and typed refs on supported properties.
Resolver v1 returns resolved literal/style values; advanced alias/theme and token management UI come later.
Asset/font metadata is readable/listable through MCP; no resource management UI is required to use the MVP.
Minimal component/variant is a post-MVP phase; no complex override or inheritance yet.
Responsive v1 is only the agent choosing a finite viewport and layout within the published row/column scope.
No commitment to automatically emulate CSS media queries or every responsive website behavior.

### Responsive opt-in — proportional sizes and breakpoints

- A child node takes `width` and `height` as a decimal percentage from `0%` to `100%`, along the parent's determined axis. The root frame keeps a numeric width and uses the viewport when rendering; `hug` height takes the content height. Min/max are in pixels.
- A container may declare up to eight `layout.breakpoints` in increasing `maxWidth` order. Choose the first layout satisfying `viewport.width <= maxWidth`, using the default layout beyond the thresholds. Each replacement layout is complete and breakpoints do not nest.
- The backend, layout binary, render and inspect use the same rule. Keep the authored revision and update the backend with the layout binary in the same image. See the [responsive contract](docs/CONTRACT.md#responsive-layout).

### Task04 — row wrap approved before implementation

Scope as in the Task04 proposal; the user explicitly approved "Approved, implement row wrap".
`layout.wrap` is a literal boolean, default false and omitted from serialization when false.
Missing/false keeps the old behavior for row/column/free; true is only for page/frame/group
containers with row mode. A wrong type (null/string/number/ref) is rejected; storage still rejects unknown keys.
A wrapping row needs a finite numeric width >0 or fill through a chain of fill ancestors up to a numeric width >0;
missing/hug width or only min/max is not definite. Direct layout fixes the selected frame
with a finite viewport; storage checks every theme and fixes the root frame width as fixed, height fixed or hug.
A direct child with fill width needs an explicit minWidth >0 (a dimension ref that resolves validly);
fixed/hug children are valid, and this guard does not apply to fill text inside a card column.
An empty row is valid; keep the existing fill-in-hug guard on both axes.

Min/max finite 0…100000, min≤max on both axes; equality is valid. Fixed outside the
interval still clamps. A total of minima exceeding the viewport is not rejected: an oversized child still overflows.
Storage validates the final candidate, every theme and update/token/theme/reparent atomically;
a VALIDATION_FAILED error carries the field, with no partial snapshot/revision/log/idempotency.
The direct guard runs before measurement, error LAYOUT_INVALID; a wrong type deserializes as INVALID_JSON.

Taffy mapping FlexWrap::Wrap + AlignContent::START only when wrap is true and align-items START;
the default no-wrap is unchanged. Sibling order is unchanged; gap between cards and between rows.
Fill grow1/shrink1/basis0 divides per row; the last row may fill the whole width, no Grid.
Hug height = padding + sum of each row's max height + row gaps; fixed/min/max still clamp,
and vertical overflow is possible. Keep the cache-only callback, batch measurement per pass, at most 6 passes,
the existing text cache key/line list/renderer and pixel/node/depth budgets; input immutable.

AC: new fixture 1040→3 cards/1 row, 640→2+1, 390→1+1+1, min280/gap16/padding24;
geometry row hug 80/156/212, long/unbroken text 128/224/320, no overflow of
frame/card/text and no overlap. Boundaries 920/919/624/623; fixed row height align start,
nested definite width, fixed/hug children, equal min/max/clamp, oversized-min overflow
control, omitted/false no-wrap compatibility, repeat cache 0 batch and immutable input.
Negative direct/storage guards include wrong mode/type, indefinite width, missing/nonpositive
fill minimum, min>max on both axes; token/theme/update/reparent and late invalid op rollback.
MCP temp root create/apply/read/context/reflow/inspect/render at three viewports, restart keeps
wrap/revision/bounds/PNG; viewer GET-only uses the shared scene/render at 1040 and 390.
Keep the full pipeline 11 and the old E2E 15 groups, adding wrap; run layout tests, full storage offline
fault-injection (2 threads), server wrap + multi_viewport/skill_layer, E2E, strict library
Clippy for layout/storage/server, fmt, JS syntax/format, diff/size checks (code ≤250 lines).

No DB migration: props JSON is additive; DB schema 2, snapshot/API/scene schema 1 are unchanged.
The scene/job key includes the resolved input and the startup pipeline fingerprint; building the new layout and
then restarting the new backend changes the cache identity. Backend/layout must be upgraded together; an old
engine may drop unknown fields, and no mixed-version/downgrade/hot replacement is promised. The task does not
restart 3847, change renderer/web behavior, the real demo or the no-wrap golden.
No %/breakpoint/column-wrap/wrap-reverse added (align/justify added later in P08a). Wrap does not fix a dashboard by itself:
sidebar/body minima, fixed chart/progress and fixed text heights still need separate authoring.

Out of scope: user editing, drag/drop node editing, undo UI, chat, terminal and AI provider API.
Out of scope: multi-user collaboration, cloud hosting, Figma plugin and a full Figma clone.
Out of scope: full CSS/Grid, rich text, vector path editor, interactive prototypes (except declarative navigation links `links`/`flowStart`: click/hover → navigate/overlay/back/close) and animation (except declarative `animations` on a node: opacity/translate/scale/rotate/fill; load/hover/click; no layout, no scene transitions).
Hover can use `triggerNodeId` on each animation to take the hover region from an
ancestor group/frame in the same root frame. Each node keeps its own keyframes and timing.
Preview shares the hover region across all tracks from the same source, reversing from the
current progress when leaving the region. MCP publishes the hover sources for the agent to implement.
Do not build a Postgres adapter, a multi-service architecture or general event sourcing before they are needed.

## 3. Architecture flow and data ownership

```text
Codex / Claude ── MCP Streamable HTTP ─┐
                                     v
                              Rust backend
                          domain + revision queue
                           /                  \
                  SQLite + assets       compute/render jobs
                           \                  /
                            HTTP snapshot + SSE
                                     |
                           React / Konva viewer

Rust compute jobs <── measure/render protocol ── headless browser worker
Viewer and worker share the web-renderer module + font manifest + scene version.
```

The authored model is the data the agent writes: tree, properties, layout rules, references and text.
The computed scene is the result for a specific snapshot revision: bounds, lines, transforms and diagnostics.
Rust outputs the computed scene; the viewer does not run a second layout engine of its own.
The shared web module draws the scene and provides text measurement; it does not modify the authored model.
Taffy is a candidate for row/column layout, with custom measure; Taffy does not measure or shape text itself.
Do not return placeholder measurements as a computed result when the worker or font is not ready.
A scene not yet computed must return `pending`, or `failed` with a clear error; the authored snapshot stays readable.
An old job may complete but must not replace a newer scene the viewer is requesting.
Cache by project/revision/frame/viewport/theme/state and pipeline version; losing the cache can still be rebuilt.

## 4. Storage and schema v1

### Engine decision and re-evaluation conditions

The workload is a design with tree/refs/revision and batches of many entities that must commit together, with frequent snapshot reads.
Multiple agents calling the same backend go through one writer queue; this does not mean multiple independent writers into the file.
SQLite WAL is the decided MVP choice; benchmarks verify performance and operating limits, and are not a step waiting on an engine choice.
This decision does not claim SQLite is absolutely optimal; the alternatives below are kept as a basis for re-evaluation when measurements exist.
The agent/runtime/chat are outside the app; the MVP needs no agent-memory or vector database to keep the design.

| Candidate | Assessment against the design contract |
| --- | --- |
| [SQLite WAL](https://www.sqlite.org/wal.html) | Transactions/constraints/queries fit snapshot + revision + idempotency; one writer, queue and checkpoint need measuring. |
| [redb](https://github.com/cberner/redb/blob/master/docs/design.md) | A real competitor: pure Rust ACID KV/MVCC; still one sequential writer, tree and secondary indexes must be designed by hand. |
| JSON atomic replace or envelope + op log | Feasible; durability, recovery, compaction and multi-file consistency must be guaranteed by hand, not assumed simpler. |
| [DuckDB](https://duckdb.org/docs/stable/connect/concurrency) / [LanceDB](https://docs.lancedb.com/faq/faq-oss) | DuckDB is aimed at OLAP analytics; LanceDB at retrieval/fragments, and neither directly replaces the design-document transaction contract in one file. |
| [AgentFS](https://github.com/tursodatabase/agentfs) | Real, uses SQLite for the agent runtime's filesystem/KV/tool calls, still beta; does not itself provide the nodes/refs/revisions semantic contract. |
| [Memvid](https://github.com/memvid/memvid) | Real, single-file recall/memory; the multi-entity transaction guarantee needed for design is unverified. |

Revisit when the write queue is measured as saturated, or when the tree/index representation is a bottleneck for a large project.
If multiple backends/machines need to write together, evaluate a server database; do not put SQLite WAL on a network filesystem.
Only change the engine after measurement and a separate decision; do not build two backends at once or promise migration in the MVP.

### Persistence, durability and assets

From M1, the project slug contains only allowed characters, with a length limit; a path from the client is not accepted as a slug.
The backend canonicalizes slug/path by the same rule when creating and opening a project; the project root is resolved inside the data root.
Block `..`, separators in the slug and symlinks leading outside the root; every path inside a project must pass a containment guard against the canonicalized project root.
A project is initialized only when the corresponding API is called; this PLAN does not create `~/.fagent`.
Each project has the structure:

```text
~/.fagent/
└── <project>/
    ├── db.sql          # SQLite binary
    ├── db.sql-wal      # WAL sidecar when present
    ├── db.sql-shm      # shared-memory sidecar when present
    └── assets/         # every asset/font of the project
```

The project ID is stable and independent of the slug; the DB manifest holds the ID, slug, schema version and current revision.
Only one process may own the data root; starting a second instance must fail clearly with a lock.
Each project has a single writer queue, reads use their own connection; WAL and foreign keys are on.
The file `~/.fagent/<project>/db.sql` is a SQLite binary, not an SQL dump; the sidecars `db.sql-wal` and `db.sql-shm`, if present, sit next to `db.sql` in the same project root.
Bundle/pin a SQLite with the WAL-reset fix: >= 3.51.3 or a verified backport such as 3.44.6/3.50.7.
Check the runtime version actually used at build/test time; do not rely on the operating system's SQLite.
Fix `synchronous=FULL`, `foreign_keys=ON`, a finite busy timeout and a checkpoint policy through the storage spike.
The busy timeout is bounded; a busy error must be distinguished from a revision conflict and a validation error.
Measure the checkpoint threshold/WAL growth and the ability to checkpoint forward after readers finish; do not force a checkpoint on every batch.
Do not hold a SQLite transaction, including a read transaction, across SSE, text measurement, render, asset loading or sending over the network.

| Table | Main columns and responsibility |
| --- | --- |
| `project_meta` | One row: `project_id`, `slug`, `schema_version`, `current_revision`, timestamps. |
| `nodes` | `id`, `parent_id`, `sort_key`, `kind`, `props_json`; parent FK, valid root page. |
| `styles` | `id`, `kind`, `name`, `value_json`; primitive style with a schema version. |
| `tokens` | `id`, `name`, `type`, `value_json`; the MVP has only literals and binding by ID of the correct type. |
| `assets` | `id`, `content_hash`, `media_type`, `byte_size`, `relative_key` relative to `<project>/assets/`, dimensions/status. |
| `fonts` | `id`, family/weight/style, `asset_id` or bundled ID, hash/license metadata if any. |
| `commits` | `revision` PK, `base_revision`, `batch_id`, `ops_json`, `summary_json`, timestamp. |
| `idempotency` | `key` PK, request hash, committed revision, response JSON; published retention. |

`props_json` holds geometry/layout/style refs/text/image refs per the discriminated schema of the `kind`.
A page is a root node; a frame/group holds children through `parent_id` and a stable `sort_key` order.
The index `(parent_id, sort_key)` serves reading children; the candidate tree in RAM is validated before commit.
SQLite does not optimize traversal of every tree by itself; representation/index/serialization must be measured separately from engine overhead.
Do not switch to one large document blob based on engine research alone without measuring the update/reparent workload.
V1 uses schema-backed JSON and validation; no per-property tables or generic EAV yet.
The current node tree is a materialized authored snapshot; `commits` is the op log for replaying notifications/diffs.
Updating the snapshot, incrementing the revision, writing the commit and the idempotency result happen in the same transaction.
There is no need to store a full snapshot for every commit; the system is not required to rebuild the DB from the op log.
Log/idempotency retention must have metadata so the client knows when to refetch or that a key has expired.
Versioned migrations run before the project is opened for writes; a migration error keeps the original DB and reports a specific error.
Every asset/font binary of the project lives in `~/.fagent/<project>/assets/`; `relative_key` resolves from this directory, and no absolute path or traversal is accepted.
The containment guard checks that the canonicalized path is inside `assets/` of the right project and inside the project root, even with symlinks; no reading/writing into another project.
Asset binaries are not atomic with SQLite: store immutably by content hash, write to temp, fsync and then atomically publish and fsync the directory before the DB ref commits.
Only ACK an import after the binary and metadata are durable; a crash before the DB commit may leave an orphan.
Deferred GC must keep the assets of the revision manifest and the render/export handles in use; the MVP needs no complex background GC.
Backup uses the [SQLite backup API](https://www.sqlite.org/backup.html) or a valid quiesce/checkpoint; do not copy the live main file.
Backup/export covers the project folder: a snapshot of `db.sql` at one revision, manifest hashes and all the binaries in `assets/` of that revision, kept until completion.
Active sidecars are not an independent backup; create a consistent DB snapshot with the SQLite mechanism above, and do not copy the whole project folder while it is being written and call it a valid backup.
Restore checks the manifest/hashes to detect missing binaries; a successful DB snapshot alone does not prove a consistent project backup.
Export/import is a versioned bundle of the project folder, containing a `db.sql` snapshot with authored data/refs, the asset manifest and the needed assets/fonts in `assets/`.
Import/restore validates schema/hash/path and containment before activating the target project folder; import creates a new project, keeping node IDs but restarting the revision.
Fix the bundle format in the hardening phase; do not use an SQL dump as the public interchange contract.

## 5. Transactions, revisions and synchronization

Each batch applies to one project and one expected revision; success increments the revision exactly once.
In the writer queue: look up the idempotency key first, then check the revision, validate the candidate and commit.
The same key + the same request hash returns the old result even if the expected revision is now stale.
The same key + a different payload returns `IDEMPOTENCY_KEY_REUSED`; no part of the batch is executed.
The request hash uses the canonical payload, including project, expected revision and operations.
From M1, validation checks schema, batch limits, IDs, tree cycles, refs, parent kind, finite values and supported layout.
A token/style/asset/font ref must have a record; a wrong type or unsupported binding is rejected, and the ref is never silently changed.
An asset/font record that exists but whose binary is not yet ready has its own missing status; aliases are not part of the MVP.
Structural validation happens before commit; measure/layout/render run from an immutable snapshot outside the transaction.
A render error after commit does not roll back the authored design; the response/readback shows a separate render status.
Only after a successful commit do we enqueue compute and publish the event; a crash after commit can still be replayed from the log.
A client that lost the response can retry with the same key; no second node is created and the revision is not incremented again.
A revision conflict returns the current revision along with guidance to re-read; the backend does not silently merge changes.

SSE is not the only source of data: the HTTP snapshot and the committed revision are the trusted marks.
The initial snapshot contains `projectId`, `revision`, the authored scope and the corresponding scene state.
The event connection has a cursor `projectId:revision`; the server replays every commit newer than the cursor still in the log.
Register the stream before taking snapshot R, then replay from the durable log after R so no event falls between the two steps.
The client ignores revisions already processed; on a gap it must replay, or refetch when the log is gone.
`reset_required` states the current revision when the cursor is for the wrong project, past retention or invalid.
The `scene_ready` event carries the source revision and scene key; it does not replace the authored model's revision.
After reconnect the viewer only shows a scene matching the selected snapshot; pending/stale states are shown clearly.
Heartbeat, buffer limits and a short reconnect backoff with jitter avoid holding a dead connection forever.

## 6. API and MCP contract

The HTTP API is versioned `/api/v1`; the MCP endpoint `/mcp` binds to localhost by default, with origin control.
The MCP schema uses a clearly typed JSON Schema, required fields, enums, bounds and a response/error envelope.
Do not put absolute filesystem paths or secrets into tool results, logs sent to clients or diagnostics.
Project listing returns only ID/slug/name/revision; an asset returns an opaque ID or a controlled local HTTP URL.
M1 fixes outline/context at most 200 nodes, depth 8, context 64 KiB; resource list at most 100 items.
Batch/pixel budgets are fixed from the spike; every limit has a server cap, truncation or its own error.

| MCP tool | Main input | Main output |
| --- | --- | --- |
| `list_projects` / `create_project` | cursor or slug/name | Project refs, revision 0 for a new project. |
| `get_outline` | project, page/frame, depth, limit | Condensed tree, IDs/kinds/names, explicit truncation; cursor added in M3. |
| `get_subtree` | project, node, depth, property filter | Authored subtree at a consistent revision. |
| `get_design_context` | project, frame, viewport, budget | Semantic tree, resolved literal/style values, token/asset/font refs, diagnostics. |
| `list_tokens` / `list_styles` / `list_assets` / `list_fonts` | project, IDs/filter, limit | Typed values or metadata/hash/readiness at a consistent revision; no binary returned. |
| `apply_batch` | project, expectedRevision, idempotencyKey, operations | committedRevision, changed IDs, render status. |
| `import_asset` | project, bounded binary/resource input, media type | Validated asset ref; no arbitrary local path accepted. |
| `render_frame` | project, revision, frame, viewport, theme/state, scale | PNG resource or job ID, provenance, readiness/errors. |
| `inspect_layout` | same scene key, node scope | Computed bounds/lines/rules and diagnostics. |
| `find_nodes` | project, page/frame/node scope, kinds/nameContains/textContains/ids, limit, cursor | Flat list of matches with ancestor path, totalMatches, explicit truncation. Read-only addition, approved by the user 2026-10-28 (Task06). |
| `get_changes` | project, sinceRevision, limit, budget | Commits + changed ids classified against the current snapshot; a conflict carries `changedSince`. Approved by the user 2026-10-28 (Task07D). |

The first version of `inspect_layout` is only bounds/status; deep explanation and diff tools open in a later phase.
Minimum HTTP: list/create project, GET snapshot, GET events, GET asset and GET render job/resource.
The viewer only uses HTTP reads; it calls no mutation and needs no MCP client integrated into the browser.
Scene/snapshot return `schemaVersion`, `revision`, scope and `truncated`; no silent cutting by token budget.
Context is outline first by default, subtree on demand; not always the whole project or base64 of every image.
MCP PNG has a resource ref/mime/dimensions, and may be image content depending on client capability, not just a file path.

First batch example, with IDs created by the caller for immediate use in the same batch:

```json
{
  "projectId": "p_demo",
  "expectedRevision": 0,
  "idempotencyKey": "demo-first-frame-v1",
  "operations": [
    {"op":"create_node","id":"page_1","kind":"page","parentId":null,"props":{"name":"Home"}},
    {"op":"create_node","id":"frame_1","kind":"frame","parentId":"page_1","props":{"width":800,"height":600,"layout":{"mode":"column","padding":24,"gap":16}}},
    {"op":"create_node","id":"text_1","kind":"text","parentId":"frame_1","props":{"text":"FAgent","width":"fill","height":"hug","fontId":"font_bundled","fontSize":32,"lineHeight":40}}
  ]
}
```

The v1 ops: create, update properties, reparent/reorder, delete subtree, upsert literal token/primitive style.
Token/style upsert through `apply_batch` uses the same validation/revision/idempotency; no arbitrary JSON Patch.
Operations are processed in order in the candidate model; a forward ref is only allowed if explicitly specified.
Success response: `committedRevision: 1`, changed IDs, `renderStatus: pending`, diagnostic refs if any.
Error envelope: `code`, `message`, `retryable`, the current revision if relevant, and details of the failing field/op.
Main codes: `VALIDATION_FAILED`, `REVISION_CONFLICT`, `IDEMPOTENCY_KEY_REUSED`, `CONTEXT_LIMIT`, `RENDER_FAILED`.
Geometry uses logical pixels; authored x/y are parent-local, and computed bounds must record the coordinate space.
Render/inspect use a frame-local origin (0,0) with an explicit transform; canvas viewport pan/zoom is not mixed into the design.
Viewport/theme/state are always resolved to concrete values in provenance, even when the caller uses defaults.
Authored and computed live in separate fields; computed width/line breaks are never written back into authored props.

## 7. Layout, typography and render readiness

The spike uses a fixed bundled font fixture, Latin text with diacritics, multi-line wrapping, images and nested row/column.
Check the font license before bundling; system fonts are not the default for a reproducible fixture.
The worker loads the font with the right hash/weight/style, and waits for `document.fonts` and image decode before measuring or capturing.
A missing font uses the declared fallback and returns a diagnostic; strict render mode must fail readiness.
A missing/broken asset has a labeled error placeholder in the viewer; a strict PNG does not report success as if the asset were complete.
One shared wrapping/line-break implementation provides the measurement and the line list for the scene.
Do not let the Taffy callback and Konva Text compute wrapping independently from the same string.
The frontend draws the computed line list/positions; the shared module keeps font loading, measurement and drawing consistent.
The Taffy measure callback is synchronous and cannot await a browser RPC; no blocking roundtrip per leaf.
M0 tries batch premeasurement and a cache of text/style/font hash/width/renderer version; a pass missing the cache collects constraints.
Rust sends the whole batch of misses then reruns a bounded layout; the scene is only published when every needed measurement is ready.
This is a protocol that must be proven through fixtures; if the constraints do not converge, decide again after the spike.
The layout/measure loop has a bounded number of passes, a timeout and non-convergence diagnostics; it does not block the writer queue.
The spike must check width constraint → wrap → hug height and fill/hug in a nested container.
Overflow/clip, rounding, baseline and line-height rules must be written as fixtures before expanding.
Taffy is only locked after custom measurement and this computation loop satisfy the real fixtures.

The render result carries the project ID, revision, frame ID, viewport, theme/state and the resolved scale.
Along with `sceneVersion`, `rendererVersion`, browser/runtime version and font/asset manifest hashes.
Along with the readiness of each font/asset, diagnostics, error code and source scene key; a job has a timeout/cancel.
The viewer and PNG share the same scene, font manifest and renderer version; a mismatch must invalidate/refetch.
A PNG may complete for an old revision when the caller requests an exact revision that is still available; label the provenance.
V1 does not promise to render every historical revision: if the requested snapshot is gone, return `REVISION_UNAVAILABLE`.
Render concurrency/pixel budgets are limited; a worker crash only fails/retries the job and loses no committed design.

## 8. Read-only viewer

- Use the decided visual: white topbar and panels, light gray canvas, dark high-contrast text, thin dividers; a blue accent for selection/focus. Keep the UI compact and neutral in the Figma editor direction so the design stands out, without requiring a pixel-perfect clone.
- Layout of topbar, layers on the left, main canvas and a read-only inspector on the right; panels can be collapsed when viewing space is needed. Status/revision are presented compactly, while still clearly distinguishing errors and a scene that is pending.
- Choose project → page → frame; choose a finite viewport suitable for the frame and the published responsive scope.
- Canvas pan/zoom/fit; this state is local view state and creates no design revision.
- The layers tree selects/highlights nodes; basic inspect of name, kind, authored props and computed bounds when ready.
- No design-editing input, resize handle, mutation shortcut, property editing or undo history.
- Loading/pending/reconnecting/missing font/missing asset/render failed must have distinct states.
- Show the revision being viewed and indicate when waiting for a new scene; do not attach a new revision to an old image.
- Reload keeps a valid project/page/frame; if the target was deleted, choose a clear fallback and do not crash.
- Show semantic design context as basic inspect; a PNG does not replace node data for the agent.
- Interaction with the design is only select/highlight, pan/zoom/fit and viewport choice; no resize handle, user editing, chat or terminal. No dark theme/toggle yet; do not create a separate design system or fake screenshots to illustrate the plan.

## 9. Milestones by slice and acceptance criteria

### M0 — pipeline spike before expanding

Create the smallest Rust skeleton + web module + small worker, without building the whole backend/extra framework.
A one-frame fixture with wrapped text, a rectangle and an image; try custom measure with the Taffy candidate.
AC: the viewer and PNG use the same computed scene/font/renderer manifest; text is not wrapped a second time.
AC: changing the width changes the line list/hug height correctly per the fixture; a font/asset not ready does not give strict success.
AC: prove batched measurement/cache with a synchronous callback and bounded relayout, not one RPC per leaf.
Targeted check: one font fixture with diacritics, two widths, one slow image decode and one missing font.
Output: layout engine/protocol decision, runtime/version pin, fixtures and verified layout limits.
The storage spike verifies the performance of the decided SQLite; record the small/large fixture scale, hardware and latency budget before benchmarking; do not put in fake pass numbers before measuring.
Measure batch create/update/reparent on two tree scales: p50/p95, bytes written, CPU/RAM, query/serialization and commit time.
Measure 1/4/8 clients sending batches while the viewer reads: queue wait, revision conflicts, WAL growth/checkpoint and engine overhead separately.
Required correctness: consistent snapshot, valid tree/refs, correct revision/idempotency; latency results compared with the recorded budget.
Without passing M0, do not expand layout features or create a general renderer abstraction.

### M1 — the agent creates a design and sees it immediately

Build project create with slug/path guard, SQLite schema, atomic/idempotent apply batch, snapshot, SSE and the viewer frame.
Connect `render_frame` through the M0 worker; add outline/context with caps, resource reads and literal token/style upsert/resolution.
AC: MCP creates page/frame/text/rect/image → commit → the browser shows it → readback/PNG match the revision.
AC: the viewer uses the decided light tone in the Figma editor direction: white topbar/panels, light gray canvas, dark high-contrast text, thin dividers, a blue accent for selection/focus; layout of topbar + layers on the left + main canvas + read-only inspector on the right, panels collapsible when needed and compact status.
AC: the viewer only allows select/highlight/pan/zoom/fit/viewport choice for the design; no resize handle, user editing, chat or terminal; no dark theme/UI toggle needed yet.
AC: MCP reads nodes/layout, resolved styles/tokens and asset/font metadata; changing a literal token updates the dependent scenes.
AC: the project is created with the correct structure `<project>/db.sql` and `<project>/assets/`; restarting the backend and reloading the browser still shows the old node IDs, content, asset refs and revision.
AC: an invalid batch/missing ref/wrong type leaves no commit; retry with the same key does not increment the revision; a wrong slug/path is blocked.
Targeted check: one E2E MCP → DB → viewer → render → restart, using a temp project in a test root.
Output: the first local running version, instructions for running Rust/viewer/worker and reproducible fixtures.

### M2 — consistency and recovery in real use

Deeply verify the expectedRevision/idempotency/atomicity contract that exists from M1; complete SSE replay, job guard and resource errors.
AC: two batches with the same base revision, only one wins; the loser receives a conflict and can re-read.
AC: retry with the same key after a lost response returns the old result; a different payload with the same key is rejected.
AC: disconnecting SSE around a commit then reconnecting receives every change; a cursor past retention can refetch.
AC: a job R completing after R+1 does not overwrite the current scene; a missing font/asset has the right status.
Targeted check: transaction integration, conflict/retry, SSE reconnect/gap and worker failure fixtures.
Add a workload with a crash before/after commit and a retry on a lost response: no partial batch, no double revision, no ACK that loses data.
Crash around asset publish/DB commit: a committed ref has a durable binary; orphans are recognized, and no asset in use is deleted.
Output: the viewer is stable across reconnect/restart; not just a happy-path demo.

### M3 — advanced context and resolution within the published scope

Add pagination/optimization of context budgets, token alias/theme resolution, explanatory inspect and the missing layout constraints.
AC: outline/subtree/context agree on the revision; truncation has a cursor; authored/computed are clearly distinguished.
AC: when adding an alias, block cycles/wrong-type refs before commit; a theme/alias change invalidates exactly the dependent scenes.
AC: nested row/column, fixed/fill/hug, padding/gap, overflow and text line-height fixtures all pass.
Targeted check: semantic contract, dependency invalidation and font/render regression with fixed fixtures.
Output: a list of supported features and structured errors for the unsupported cases.

### M4 — data hardening and conditional expansion

Add versioned migration/backup/export/import; deep inspect/diff or component/variant only when really needed.
AC: project folder export/import round-trip keeps authored data and resource hashes in `db.sql` together with `assets/`; a failed migration does not damage the original DB.
Backup/restore workload of the project folder with writes and render: the DB revision, resource manifest and all needed binaries must match.
AC: restore opens the project at the captured revision, with all hashes checked; GC does not break a running backup/export.
AC: extend failure tests for the containment/slug guards that exist from M1, symlink/archive traversal and assets over the limit.
Targeted check: one migration fixture, bundle round-trip, malformed archive and containment guards.
Output: local operations documentation, known limits, schema upgrade rules and project recovery.

## 10. Verification approach and MVP completion conditions

The MVP is complete when M0–M2 pass; M3/M4 can be assigned in parts as needed, and do not block seeing designs.
Each milestone records the commands run, fixtures, results and revision/provenance; do not write "pass" if it has not been run.
Prefer unit tests of the validator/resolver and integration/E2E at the seams with data or display consequences.
Compare the scene/line list first, use PNG fixtures on a fixed runtime with a published tolerance when needed.
Do not treat a single pixel snapshot as evidence of persistence, font readiness or SSE without lost events.
Do not repeatedly run the full build/test suite if the change only needs a targeted check; limit render concurrency.
Do not call an AI provider to test the backend; an MCP client fixture is enough to check the agent contract.
Early risks: typography, font fallback, wrap/hug convergence and the worker runtime; M0 handles them before expanding.
Next risks: revision/render race, reconnect gap and asset lifecycle; M2 has its own AC for each case.
Storage risks: writer saturation, long read transactions causing WAL growth and DB/assets durability mismatch; benchmark/recovery are checked separately.
Do not add a huge deployment/security checklist; local by default, path containment and errors that do not leak secrets are mandatory.

## 11. Technical sources cross-checked

- [Official Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk): the `rmcp` foundation, HTTP/stdio.
- [MCP tools specification](https://modelcontextprotocol.io/specification/2025-11-25/server/tools): schema and tool results.
- [Taffy](https://docs.rs/taffy/latest/taffy/): layout and custom measurement; does not replace a text engine.
- [SQLite WAL](https://www.sqlite.org/wal.html): concurrent readers/writer, one writer at a time.
- [Konva state management](https://konvajs.org/docs/data_and_serialization/Best_Practices.html): keep application state separate.
