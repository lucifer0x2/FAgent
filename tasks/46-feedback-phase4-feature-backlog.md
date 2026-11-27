# Task 46 — Agent feedback, phase 4: feature backlog

**Priority (user, 2026-11-27):** backlog. Not placed in any order; the user picks items one at a time.
**Status:** Not assigned. Packaged 2026-11-27 from a read-only trace against `main` at `7d71753`. Items are named `FB-n` after the maintainer's private feedback log (see [Task 43](43-feedback-phase1-small-fixes.md) "Context"); examples are synthetic.
**How to use:** this file is an index, not one handoff. When the user schedules an item, package it as its own task file (goal, owned scope, branch, AC, verify, outputs), settle its design questions with the user first, and read `docs/REJECTED-APPROACHES.md` before choosing an approach. Mark the item here with the new task number.

## Items

| Item | Feature | Size | Notes |
| --- | --- | --- | --- |
| FB-15 | Text `maxLines` and `textOverflow:"ellipsis"` | M–L | See 1 |
| FB-39 | Dashed strokes | M–L | See 2 |
| FB-41 | Duplicate a node subtree with new ids | L | See 3 |
| FB-24 | Slots: an instance holds authored children | L | See 4 |
| FB-25 | Breakpoints that change child props | M–L | See 5 |
| FB-29 | Notes on virtual nodes inside instances | L (S variant) | See 6 |
| FB-16, FB-48 | Read and render past revisions | — | Already [Task 39](39-render-past-revision.md); Task 43 slice C only names the current revision |
| FB-49 | Render frames above 16 Mpx | L | See 7 |
| FB-8 | Import assets by URL; larger assets | L | See 8 |
| FB-44 (rest) | Several strokes on one node (border + focus ring) | L | Model-wide; the state shadow is Task 45 item 3 |
| — | Small optional improvements | S | See 9 |

1. **Text truncation (FB-15).** Today `capabilities.rs` ~42 says "no … ellipsis or maxLines". Validation in `validate/text_props.rs` and `props_catalog.rs` (`maxLines` ≥ 1, `textOverflow` `clip|ellipsis`); layout in `crates/layout/src/text_lines.rs` and `model/text.rs` (truncate to `maxLines`, hug height = lines × lineHeight, re-measure the last line with "…" using the fit loop of `crates/text/src/wrap.rs`; a new measure request and a scene/renderer version bump); diagnostics must not flag truncated lines. Questions: the ellipsis glyph when the font lacks U+2026; per grapheme or per word. `docs/REJECTED-APPROACHES.md` forbids new features in the Chromium worker and the Canvas2D fallback: decide whether those paths get it or the feature is Vello/native only.
2. **Dashed strokes (FB-39).** `validate/stroke.rs` ~28–40 accepts color/width/align only; the model is `crates/layout/src/model/stroke.rs`. Vello paints borders as filled rings (`crates/paint-vello/src/stroke.rs`); a dash can be built with kurbo's dashed stroke-to-fill on the centreline, offset for inside/outside, without changing the `Painter` trait. Questions: `style: dashed|dotted` (maps to CSS) or `dash:[on, off]` (SVG-like); uniform width only; same Chromium/Canvas2D question as item 1.
3. **Duplicate a subtree (FB-41).** No copy operation exists (`crates/scene-input/src/model/requests.rs` ~4–60). A new JSON op `duplicate_subtree {id, prefix|newId, parentId?, index?}` applied in `validate/candidate.rs` (which holds the node map), plus OCD `@dup`. Expanding it in the server before storage was rejected during the trace: the compiled body would depend on stored state and break idempotency. Touches node-memory batch rows, the page guard and delta validation. Questions: id scheme within 128 characters; remapping links/`flowStart` inside the copy; copying a master (new name?); notes not copied; return an id map.
4. **Slots (FB-24).** Instances are leaves (`crates/scene-input/src/model.rs` ~41–43; writes under `/` ids fail `INSTANCE_CHILD_READONLY`). Sketch: a `slot` marker on a master descendant, authored instance children with a slot target, re-parented by the expander under `<inst>/<slotId>`; validation, write guards, outline `childCount`, delta facts and expansion budgets follow. Needs a design document first.
5. **Breakpoints for child props (FB-25).** A breakpoint swaps only the container's layout rule (`crates/layout/src/tree.rs` ~20, `model/layout.rs` ~62). Sketch: a per-node responsive list (`width`/`height`/`x`/`y`/`visible`) chosen by the same viewport match, validated in `validate/breakpoints.rs`, with fill/percent definiteness re-checked per breakpoint. Today agents keep a second master per breakpoint.
6. **Notes on virtual nodes (FB-29).** Rejected in three layers: the schema id pattern (`crates/server/src/schemas/operations.rs` ~44–58 uses `id()`, not `node_id()`), node-memory `anchor` (`ops/notes.rs` ~21–29) and scene-input `check_note` (`validate/notes.rs` ~118–152); the page guard has no page for a virtual id. Full support needs anchor semantics when the master child disappears (cascade, orphan or re-anchor to the instance). Cheaper variant (S–M): anchor the note to the instance and store a `virtualPath` the viewer and reads show.
7. **Large renders (FB-49, FB-13).** Task 43 slice C makes the 16 Mpx limit visible. Beyond it: tiling or automatic downscale with the actual scale reported in file metadata. `vello_cpu` targets are limited to `u16` sides (`crates/paint-vello/src/export.rs` ~35–39) and exports have a 48 MiB file budget (`crates/server/src/jobs/export.rs` ~123–131).
8. **URL import and larger assets (FB-8).** `import_asset` has no URL source ("No URLs", `schemas/import.rs` ~14) and a 10 MiB cap per asset (`crates/storage/src/asset_import.rs` ~84). A URL fetcher needs an SSRF-safe design (allowed schemes, no private addresses, size and time limits); raising the cap touches bundle and memory budgets.
9. **Small optional improvements.** `REVISION_CONFLICT` could add `conflictKeys` (the keys that actually conflicted; `changedIds` lists everything since the base, `crates/server/src/reads/changes/conflict.rs` ~31–45) (FB-6); export TTL and budget configurable by flag or env, and a note that budget eviction can end a URL before `expiresAt` (FB-11).
