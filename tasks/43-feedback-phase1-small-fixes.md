# Task 43 — Agent feedback, phase 1: small fixes

**Priority (user, 2026-11-27):** first of the agent-feedback phases (43 → 44 → 45; 46 is a backlog). Independent of the viewer order (36 → 37 → 31/35 → 33).
**Status:** Done 2026-11-27 on `fix/agent-feedback` (slices A–D merged; not pushed). Results in "Result" at the end. Packaged 2026-11-27 from a read-only trace of the maintainer's agent-feedback log against `main` at `7d71753`. The server crates on `main` are identical to the deployed build, so every open item below still reproduces on `main` unless it says "reproduce first".
**Depends on:** nothing. Tasks 44 and 45 build on slice B.
**Slices:** four, with disjoint owned files, so each can run in its own session. One session may take several slices in order.
**Workspace:** one branch per slice from `main`: `fix/feedback-ocd` (A), `fix/feedback-instances` (B), `fix/feedback-reads` (C), `fix/feedback-storage` (D). A session that takes every slice may use `fix/agent-feedback` (already created from `main`). Do not push or open a PR unless the user asks.
**Authority:** edit the owned files, add tests, update the listed docs and MCP texts, commit on the slice branch. No deploy. Never connect to the running server or its database, and never write `~/.fagent`: reproduce with a temporary data root and a server you start yourself.

## Context

AI agents author designs through the MCP tools and record every problem they hit in a feedback log the maintainer keeps outside this repository. Items here are named `FB-n`, where `n` is the item number in that log, so the maintainer can map results back. All examples below are synthetic. Line numbers refer to `7d71753`.

Already fixed on `main` (no work; list them in the final report as "fixed on main"):

- FB-3: one `apply_batch` may carry `source` and `operations` (source ops first; `crates/server/src/ocd/apply.rs`, test `crates/server/tests/ocd_edits.rs`).
- FB-4, FB-6, FB-7: the page guard rewrite (`crates/storage/src/memory/guard.rs`, `docs/PAGE-CONCURRENCY.md`): notes on a guarded page pass in scoped mode; appending a page no longer writes `*`; the first root frame of a page is accepted unless it becomes the project's first root.
- FB-9 (b, c): OCD auto ids are hashes, not positions (`ocd/auto_id.rs`); a misspelled override key is an error with a suggestion (`ocd/instance_keys.rs`, `validate/instance_checks.rs`).
- FB-11: export files live 24 h apart from the 32-job table and the result carries `expiresAt` (`crates/server/src/jobs/export_store.rs`).
- FB-20: `find_nodes` defaults to `instances:"reference"` and reports `skippedInstanceMatches` (`crates/server/src/reads/find.rs`).
- FB-8 (mostly): `import_asset` takes an inbox `path` and copies fonts/assets from another project with `sourceProjectId`. URLs and larger files are in Task 46.

## Rules for every slice

- Write a test that fails on `main` before each fix, then fix. Keep every input that is accepted today accepted, unless the item says otherwise.
- When behavior changes, update what agents read: tool descriptions (`crates/server/src/schemas*`), skills (`crates/server/skills/*.md`), `crates/server/src/capabilities.rs` and `props_catalog.rs` texts. An agent must be able to learn the new behavior from MCP alone.
- `docs/SERVER.md` is shared: each slice edits only the sections about its own items; resolve conflicts on rebase.
- Error messages name the field and an accepted form, in the existing `StorageError::validation` / `ApiError::with_detail` style.
- Add one dated entry to `docs/agents/LOG.md` per slice (assignment, result, blockers).

## Slice A — OCD compiler

**Owned:** `crates/server/src/ocd/*`, `crates/server/skills/ocd.md`, `crates/server/tests/ocd_*.rs`; `crates/layout/src/stretch_tests.rs` for the FB-27 test only.

1. **FB-17, FB-35: same override syntax in `@set` as in `use`.** `use#x m t1="A"` and `use#x m t1={"text":"A"}` work, but `@set x t1="A"` and `@set x t1={…}` fail with `SOURCE_PARSE_FAILED Unknown key`; only `t1.text="A"` works. Cause: `partial()` in `ocd/set.rs` (~76–81) routes only dotted keys to the instance target. Fix: route a bare key that is not a catalog prop to the descendant path (`descendant_value`, `ocd/props.rs` ~137) inside the `merge:true` patch. The test at `ocd/tests_set.rs` ~84–89 asserts today's failure on purpose; replace it.
   - AC: `@set x t1="A"` compiles to the same op as `@set x t1.text="A"`; `@set x t1={"text":"A","visible":false}` equals the dotted form; other overrides of `x` stay; a misspelled key on an instance fails with a suggestion; on a node that is not an instance the error says descendant keys need an instance.
2. **FB-19: `key=null` removes a prop.** `@set n maxWidth=null` fails "Expected a finite number": a bare `null` parses as the string `"null"` (`ocd/value.rs` ~44), and `set.rs` ~41 puts null values into the non-merge op. Fix: bare `null` → `Value::Null`; nulls always go into the `merge:true` patch; `null` on a create line (`frame`, `text`, `use`, …) is an error "null only removes a prop on @set". The literal text "null" is written quoted.
   - AC: `@set n maxWidth=null fill=null layout.gap=null t1.fill=null` (t1 a descendant) compiles to one `update_node merge:true` carrying those nulls, and a round trip through storage removes them; `ocd.md`'s "`null` deletes" is now true.
3. **FB-37: bare `clip` flag.** `frame#x w=10 h=10 clip` fails "Unexpected `clip`" (`ocd/parser.rs` ~195 accepts only `row|col|free|wrap`). Fix: accept bare `clip` as `clip=true` (on create lines and `@set`). Any other bare word that is a prop name fails with "write `<key>=<value>`" (do not add a bare `visible`).
   - AC: both forms compile identically; `frame#x visible` gives the hint.
4. **FB-5 (OCD part): root frames without `h`.** A root `frame w=1440` with a `row`/`col` layout and no `h` fails in storage. Storage already accepts `height:"hug"` for row/column roots (`crates/scene-input/src/validate/root_size.rs`); the compiler writes nothing. Fix: for a root frame (parent is a page) with a row/column layout and no `h`, write `height:"hug"`; for a `free` (or layout-less) root without `h`, fail at compile time with "root frames need h=<n>, or h=hug with row/col". JSON operations are unchanged (their error already names `"hug"`).
   - AC: `frame#r w=1440 col` under a page compiles with `height:"hug"`; `frame#r w=1440 free` fails with the hint; a nested frame without `h` gets no height (unchanged).
5. **FB-27: reproduce first.** Report: a frame written without `h` (`frame#x w=24 free` inside a row with `align=stretch` and a taller sibling) does not stretch, as if `height:"hug"` were set. On `main` the compiler writes no height for frames (`ocd/defaults.rs` covers text only) and the layout stretches an unset cross size (`crates/layout/src/stack.rs` `self_align`), yet the report recurred on code identical to `main`. An earlier triage (`docs/agents/LOG.md` "Agent feedback: known limitations — 2026-11-17") noted "an unset axis treated as hug once stretched": start from that lead. Run exactly that source through `apply_batch` on a temporary server, then read `get_subtree` and `inspect_layout`.
   - If it stretches: add an OCD test (frame without `h` has no `height`) and a layout test (a `free` frame with unset height in a stretch row gets the row height), and report "not reproducible".
   - If it does not: find where the height comes from (stored props, read defaults, or the auto height of a `free` container) and fix it. If the fix belongs in `crates/layout/src/tree.rs`/`stack.rs` (slice B's files), do it after slice B merges or hand it to slice B.

## Slice B — instances, layout and component docs

**Owned:** `crates/scene-input/src/instances.rs`, `instance_paint.rs`, `validate/instance_checks.rs`, `validate/properties.rs`, `validate/layout.rs` (message only), `props_catalog.rs`; `crates/layout/src/tree.rs`, `crates/layout/src/validation/layout.rs` (message only), layout tests other than `stretch_tests.rs`; `crates/server/src/capabilities.rs`, `crates/server/src/props_doc.rs`, `crates/server/skills/author-frame.md`, `crates/server/tests/instance*.rs`; `docs/COMPONENTS.md`, `docs/INTERACTION-STATES.md`, `docs/DESIGN-RULES.md`.

1. **FB-23: `fill` inside a `free` container resolves against the root (real layout bug).** In a `free` parent, `fill` becomes `percent(1.0)` on an absolute child (`crates/layout/src/util.rs` ~41, `tree.rs` ~75). The container stays `Position::Static` unless it has breakpoints or `"N%"` children (`tree.rs` ~91–103 tests `Sizing::percentage`, which does not match `"fill"`), so Taffy sizes the child against the nearest positioned ancestor. Example: column root 1440 wide → `free` frame 24×55 → `rect w=fill h=fill` comes out 1440×N. Fix: make every `free` container `Position::Relative` (or add `fill` to the predicate; prefer the simpler rule if no golden scene moves).
   - AC: the example measures 24×55, also when the 24×55 frame is a master seen through an instance; existing layout, parity and golden tests pass, or each changed expectation is explained in the commit.
2. **FB-40: `states` on an instance root.** `use#x m states={"hover":{"fill":…}}` fails "not an instance property": `states` is in `OVERRIDE_KEYS` but not `INSTANCE_KEYS` (`instances.rs` ~13–36). Per-state merge already exists (`instance_paint.rs` ~8–25). Fix: add `states` to `INSTANCE_KEYS`; make sure the expanded root is validated with it.
   - AC: the instance root's hover fill renders with `render_frame {state:"hover"}`; `docs/COMPONENTS.md` and `docs/INTERACTION-STATES.md` no longer say instances cannot author `states`.
3. **FB-51, FB-26: `clip` on instances.** An instance root becomes a frame, which clips by default (`crates/layout/src/scene_builder.rs` ~98), and neither the instance nor an override may set `clip`. Fix: add `clip` to `INSTANCE_KEYS` and `OVERRIDE_KEYS`.
   - AC: an instance with `clip:false` gives scene `clip=false` and a child stroke past its edge is painted; an override `clip:false` on a descendant frame works.
4. **FB-31: text paint overrides.** Add `textDecoration`, `textTransform`, `textAlign`, `letterSpacing`, `textWrap` to `OVERRIDE_KEYS`; their validators exist (`validate/properties.rs` ~52). Font keys (`fontSize`, `lineHeight`, `fontId`, `fontWeight`, `fontStyle`) are out of scope unless the user approves them.
   - AC: `textDecoration:"underline"` on a master text child via an instance renders underlined; an invalid value fails under `props.overrides.<id>.textDecoration`.
5. **FB-30 (message only):** `overrides.<masterRootId>` fails as "not a descendant" (`instance_checks.rs` ~93). Make the error say the master root's props are set on the instance itself and list the instance keys. Accepting the alias or root `layout` is Task 44.
6. **FB-50 (message only):** a `w=fill` child in a wrapping row without `minWidth` is rejected (`validate/layout.rs` ~95–107, same check in `crates/layout/src/validation/layout.rs` ~63–69). Both messages name `minWidth` with an example (`minWidth=240`). The rule itself stays.
7. **FB-42, FB-43 (docs):** frames clip by default, groups do not (`scene_builder.rs` ~98); a child's outside stroke or shadow is cut by a clipping parent. State the default in the `clip` entry of `props_catalog.rs`, in `author-frame.md` and `docs/DESIGN-RULES.md`, with the fix `clip=false` on the parent. Flipping the default is not in scope.
8. **FB-5 (docs):** `props_doc.rs` ~56 and `docs/DESIGN-RULES.md` ~88 still say root frames need a numeric height; they accept `"hug"` with a row/column layout.
9. **Override lists:** `docs/COMPONENTS.md` lists 6 override keys (the code has 11, more after items 2–4); update it, `props_catalog.rs` ~106 and `capabilities.rs` ~47 from the constants.

## Slice C — reads, errors and diagnostics

**Owned:** `crates/server/src/reads/*`, `crates/server/src/node_rows.rs`, `inspect.rs`, `worker.rs` (error pass-through), `scenes.rs`, `export/mod.rs`, `jobs/input.rs`, `jobs/png.rs`, `text_diagnostics/*`, `schemas.rs`, `schemas/render.rs`, `schemas/export.rs`; `crates/scene-input/src/scene_diagnostics/overflow.rs` (the item cap only); their tests; `docs/SERVER.md` (reads, render, export, diagnostics), `docs/CONTRACT.md`.

1. **FB-52: `get_outline` ignores `properties`.** The shared tree schema (`schemas.rs` ~98) offers `properties`, but outline entries never read it (`reads/tree_nodes.rs` ~32). Fix: when `properties` is given, add a `props` object with only those keys (the default output stays prop-free).
   - AC: `get_outline {nodeId:<page>, depth:1, properties:["x","y","width","height"]}` returns them for each root frame; without `properties` the output is unchanged.
2. **FB-38: cursor bound to `properties`/`budget`.** `reads/pagination.rs` ~9–28 hashes the whole request except `cursor`, so changing `properties`, `budget` or `limit` between pages gives a generic `INVALID_CURSOR`. These do not change the entry list the offset indexes. Fix: leave them out of the hash; for the parameters that stay bound, store short per-field digests and name the changed parameter in the message and `error.field`; a revision change says so.
   - AC: page 2 with other `properties` or `budget` succeeds and is consistent; changing a filter names it; update `crates/server/tests/find_nodes.rs` ~177–198 and the "repeat every other argument unchanged" sentence in `docs/SERVER.md`.
3. **FB-13, FB-49: pixel budget errors state the limit.** `render_frame` and `export_nodes` refuse above 16,000,000 px (`crates/layout/src/lib.rs` ~37 and other copies), and the worker reply is genericized to "Render worker could not complete this representation" (`worker.rs` ~64–71). Fix: `PIXEL_BUDGET` errors carry details `maxPixels`, `requestedPixels`, `maxScale` (largest scale that fits, two decimals) and a message with the numbers; pass the code's own message through; both schemas state the limit. A hug root that grows past the requested viewport is checked after layout and fails the same way.
   - AC: 1440×6000 at scale 2 fails with `maxScale` 1.36; 1440×12000 at scale 1 fails naming both pixel counts.
4. **FB-16, FB-48: name the usable revision.** Every read/render path accepts only the current revision (`jobs/input.rs` ~41–47, `scenes.rs` ~53–57 and ~87–91, `reads/pagination.rs` ~10–15, `export/mod.rs` ~43–48). `REVISION_UNAVAILABLE` gains detail `currentRevision` and says only the latest revision can be read or rendered. Keeping older revisions is Task 39.
5. **FB-14: icon-font text reads back as `""`.** The server copies text unchanged (`reads/find/entry.rs` ~30, `reads/tree_nodes.rs` ~35) and sends raw UTF-8, so a private-use character such as U+E40D is most likely dropped by the agent's client. First confirm with a raw `tools/call` over HTTP against a temporary server and record the bytes. Then, whatever the result, add `textEscaped` (the text with private-use, format and unassigned characters written as `\uXXXX`) next to `text` and `textPreview` when the text contains such a character, in `find_nodes`, `get_subtree` and `get_design_context`. Document that input accepts both the raw character and the JSON escape.
   - AC: a text node `"\u{E40D}"` gets `textEscaped:"\\uE40D"`; plain text gets no new field.
6. **FB-32: list every overflowing text node.** The scene step keeps 12 overflow items and drops the rest (`overflow.rs` ~5, ~89), yet the hint says "All items: inspect_layout per job" (`text_diagnostics/budget.rs` ~13). Fix: keep every item in the stored job diagnostics (cap 500, with a count beyond that); context reads keep their 12-item budget; `inspect_layout` pages diagnostics (`offset`/`limit`) and its `nodeIds` filter finds any item; correct the hint and `docs/SERVER.md` ~481.
   - AC: a frame with 20 overflowing nodes → all 20 ids reachable through `inspect_layout`; the summary is unchanged in size.

## Slice D — storage, notes, assets and startup

**Owned:** `crates/storage/src/assets.rs`, `asset_import.rs`, a new image-size module, `inbox.rs` (messages), `backend.rs`, `backend/handle/inbox.rs`, `postgres/adapter.rs`, `scoped.rs`, `revision_indexes/shared.rs`, `batch.rs` (dry-run result); `crates/node-memory/src/ops/delete.rs` and batch rows; `crates/scene-input/src/validate/notes.rs`, `validate/candidate.rs`, `model/requests.rs`; `crates/server/src/startup.rs`, `svg/image_meta.rs` (reuse only), `schemas/operations.rs`, `schemas/import.rs`; the `@del` line of `crates/server/skills/ocd.md`; their tests; `docs/SERVER.md` (assets, notes, staging, edit context), `docs/POSTGRES-BACKEND.md`, `docs/SERVER-OPERATIONS.md`. If `get_edit_context` needs a change under `crates/server/src/reads/` (slice C), coordinate or do it after slice C merges.

1. **FB-1: `apply_batch_file` → `STAGING_UNSUPPORTED` on PostgreSQL.** On `main`, a server started with a data root always sets the PostgreSQL inbox root (`startup.rs` ~194–224), and then `read_inbox_batch` never reaches the adapter's `STAGING_UNSUPPORTED` (`postgres/adapter.rs` ~215–224). The fix ("read batch files from the inbox root on every engine") is recorded in `docs/agents/LOG.md` "Agent feedback: known limitations — 2026-11-17"; the later report most likely came from a build deployed before it, or from another process; say so in the report. Fix the failure modes anyway: PostgreSQL without a data root fails at startup like SQLite ("Data root is required"); log the inbox root at startup; every `STAGING_UNSUPPORTED` and `PATH_REJECTED` message names the fix (`--data-root` / `FAGENT_DATA_ROOT`; "copy the file into `inboxDir` from `list_projects`").
   - AC: a startup unit test for PostgreSQL without a data root; an ignored-by-default PostgreSQL server test of `apply_batch_file` with a `.json` and an `.ocd` file, like `crates/server/tests/postgres_import_inbox.rs`.
2. **FB-21: WebP/JPEG dimensions.** `asset_dimensions` (`assets.rs` ~183–192) parses PNG only and stores `null` for JPEG/WebP, while `crates/server/src/svg/image_meta.rs` ~56–103 already parses all three. Fix: move that parser into storage, use it at import, and let `image_meta.rs` reuse it. Existing assets with `null` dimensions are filled on read for image types (no migration). The 40 Mpx import cap applies to new JPEG/WebP imports only.
   - AC: WebP (VP8, VP8L, VP8X) and JPEG imports report width and height in `list_assets`, on SQLite and PostgreSQL; an older asset with null dimensions reads with real ones.
3. **FB-46 follow-up: note count.** The note cap is configurable, but no tool reports the count. `get_edit_context` returns `noteCount` and `noteLimit` (the count exists as `Lookup::note_count`; the limit from the configured `NoteLimits`).
   - AC: correct on the memory path and the snapshot path; documented in the tool description.
4. **FB-45: deleting a node reports the notes it removes.** `delete_node` cascades notes (`validate/candidate.rs` ~217, `validate/notes.rs` ~89–101; node-memory `ops/delete.rs` ~31–39); the ids only show up mixed into `changedIds`. Fix: `BatchResult` and the dry-run result gain `deletedNoteIds` (omitted when empty), in both the node-memory and scene-input paths under the existing parity tests; the `delete_node` description and the `@del` line in `ocd.md` say notes go with the node.
   - AC: `@note x "…"` then `@del x` returns the note id in `deletedNoteIds`, in dry run and commit.

## Verify

- Per slice: `cargo test --offline` for the touched crates (`fagent-layout`, `fagent-scene-input`, `fagent-node-memory`, `fagent-storage`, `fagent-server`), `cargo clippy --offline -p <crates> --all-targets -- -D warnings`, `cargo fmt --all --check`, `scripts/check-public-content.sh`.
- Known on `main`: `fagent-server` render tests need the Chromium render worker, and `notes::a_notes_only_commit_enqueues_no_prerender` fails without a prerender; compare failures against `main` and report them, do not hide them.
- MCP-level checks run on a server you start with a temporary data root (SQLite; PostgreSQL only through the ignored tests' own setup).

## Outputs

- Commits on the slice branch; tests for every item; updated MCP texts and docs.
- A `docs/agents/LOG.md` entry per slice and a status line at the top of this file.
- A final table for the maintainer: `FB-n` → fixed / not reproducible / deferred (with the reason), so the feedback log can be updated.

## Result — 2026-11-27

Each slice was built on its own branch (`fix/feedback-ocd`, `fix/feedback-instances`, `fix/feedback-reads`, `fix/feedback-storage`) and merged into `fix/agent-feedback`. Tests: the same 41 failures as `main` (Chromium render worker, `notes::a_notes_only_commit_enqueues_no_prerender`, two `startup_env` PostgreSQL tests), no new ones; render AC were checked on the native Vello CPU path. The PostgreSQL ignored tests passed through `scripts/task19-check.sh`.

| FB | Result |
| --- | --- |
| FB-17, FB-35 | Fixed (A): bare descendant keys on `@set` compile like the dotted form. |
| FB-19 | Fixed (A): `key=null` on `@set` removes the prop through the merge patch; `null` elsewhere is an error. |
| FB-37 | Fixed (A): bare `clip` is `clip=true`; other bare prop names ask for `key=value`. |
| FB-5 | Fixed (A, B): row/column root frames without `h` hug; `free` roots without `h` fail at compile; docs updated. |
| FB-27 | Not reproducible (A): a `free` frame without `h` stores no height and stretches; pinned by OCD, layout and end-to-end tests. |
| FB-23 | Fixed (B): every `free` container is the containing block of its children; no golden moved. |
| FB-40 | Fixed (B): `states` on an instance root. |
| FB-51, FB-26 | Fixed (B): `clip` on instances and in overrides. |
| FB-31 | Fixed (B): text paint overrides; font keys deferred (need user approval). |
| FB-30 | Message fixed (B); alias or root `layout` is Task 44. |
| FB-50 | Message fixed (B); the rule stays. |
| FB-42, FB-43 | Documented (B); flipping the clip default is not in scope. |
| FB-52 | Fixed (C): `get_outline` honors `properties`. |
| FB-38 | Fixed (C): cursors no longer bind `properties`/`budget`/`limit`; errors name the changed field or revision. |
| FB-13, FB-49 | Fixed (C): `PIXEL_BUDGET` carries `maxPixels`, `requestedPixels`, `maxScale`. |
| FB-16, FB-48 | Error fixed (C): `currentRevision`; reading older revisions is Task 39. |
| FB-14 | Server not at fault (C): a raw `tools/call` returns U+E40D intact; `textEscaped` added. |
| FB-32 | Fixed (C): jobs keep up to 500 overflow items; `inspect_layout` pages them. |
| FB-1 | Failure modes fixed (D); the report most likely came from a build deployed before the 2026-11-17 inbox fix. |
| FB-21 | Fixed (D): JPEG/WebP sizes at import; null sizes filled on read. |
| FB-46 (follow-up) | Fixed (D): `noteCount`/`noteLimit` in `get_edit_context`. |
| FB-45 | Fixed (D): `deletedNoteIds` in commit and dry-run results. |
| FB-3, FB-4, FB-6, FB-7, FB-9 (b, c), FB-11, FB-20, FB-8 (most) | Fixed on `main` before this task. |

Behavior changes agents may notice: `render_frame` over the pixel area returns `PIXEL_BUDGET` instead of `INVALID_REQUEST`; cursors have a new, shorter format (cursors issued before a deploy become invalid); a bare unknown key on `@set` is a descendant override, so a misspelled prop fails at commit with a suggestion; PostgreSQL needs a data root like SQLite. The discovery-workflow byte budget was raised to 105,000 B at the user's request; `tools/list` stays inside 32 KiB.
