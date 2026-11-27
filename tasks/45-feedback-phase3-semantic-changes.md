# Task 45 — Agent feedback, phase 3: behavior changes that need a decision

**Priority (user, 2026-11-27):** third agent-feedback phase, after [Task 43](43-feedback-phase1-small-fixes.md); may run beside [Task 44](44-feedback-phase2-instance-overrides.md) when files do not overlap (item 1 touches `crates/layout/src/tree.rs` too: run it after Task 44 item 3 or on top of it).
**Status:** Not assigned; every item waits for the user's decision. Packaged 2026-11-27 from a read-only trace against `main` at `7d71753`. Items are named `FB-n` after the maintainer's private feedback log (see Task 43 "Context"); examples are synthetic.
**Depends on:** Task 43 merged.
**Owned scope:** per item below; docs `docs/COMPONENTS.md`, `docs/INTERACTION-STATES.md`, `docs/TEXT-INK-DIAGNOSTICS-FIX.md`, `docs/SERVER.md`, `docs/VIEWER.md` (item 3 preview only), and the matching MCP texts (`capabilities.rs`, `props_catalog.rs`, `props_doc.rs`, `crates/server/skills/*.md`).
**Workspace:** one branch per item from `main`: `fix/feedback-hidden-layout`, `fix/feedback-text-diagnostics`, `fix/feedback-state-shadow`, `fix/feedback-text-spaces`. Do not push or open a PR unless the user asks.
**Authority:** as in Task 43. These items change what existing designs look like or which diagnostics they get: before coding, ask the user the item's question, state what changes for designs that already exist, and record the answer in `docs/agents/LOG.md`. Never measure on the user's real projects yourself; if numbers from a real project are needed, ask the maintainer to run a read-only count.

## Items

1. **FB-18: hidden nodes keep their layout space.**
   - Today `visible:false` becomes `opacity:0` (`crates/scene-input/src/validate/refs.rs` ~173, `validate/scoped.rs` ~66); layout `Props` has no `visible` (`crates/layout/src/model.rs` ~62–93), so a hidden child keeps its size and the gap around it. This is documented in `docs/COMPONENTS.md` ~37 and applies to every frame, not only instances. Agents expect hidden items in a row (a breadcrumb with two items hidden) to take no space.
   - Question: switch to "hidden takes no space" (Figma, CSS `display:none`)? Recommended yes. Existing designs that relied on hidden spacers will shift.
   - Fix if yes: pass `visible` to layout and set `Display::None` in `tree.rs` ~22; decide what `inspect_layout`/viewport reads report for hidden nodes (zero size at the parent's origin is recommended); skip them in paint and hit testing; keep the viewer and the Canvas2D/Chromium path in parity or document the difference.
   - AC: a row with `gap:8` and a hidden middle child lays out as if the child were absent; the same through an instance override `visible:false`; docs and MCP texts updated.
2. **FB-36, FB-47: text diagnostics on intentional clipping and small ink overhang.**
   - `TEXT_CONTENT_OUTSIDE_BOX` is a warning for every text cut by any clipping ancestor (`crates/scene-input/src/scene_diagnostics/overflow.rs` ~58–78). Because frames clip by default, the scene cannot tell an authored `clip:true` from the default. Ink overhang is allowed up to 1 px only when both advance edges are flush with the box, otherwise 0.5 px (`overflow.rs` ~6, ~111–123), so a leading "J" with ink at −1.14 px in a fill-width box warns. Both were pinned by Task 21 ("do not broadly suppress overflow diagnostics", `docs/TEXT-INK-DIAGNOSTICS-FIX.md`).
   - Question: relax both? Recommended: (a) carry an "authored clip" flag into the scene; text that fits its own box and is cut only by an authored `clip:true` gets severity `info` (code `TEXT_CLIPPED_BY_ANCESTOR`); default clips keep `warning`; (b) per-edge ink tolerance of max(1.5 px, 0.1 em) for ink-only overhang, reported as `info`; advance and clip checks stay strict.
   - Owned: `overflow.rs`, `scene_diagnostics/intent.rs`, the scene clip model in `crates/layout` (flag only), `crates/scene-wasm/src/engine.rs` call sites, `crates/server/src/text_diagnostics/*` tests.
   - AC: an overlay frame 900 high with `clip:true` and a taller column gives no warning; a "J" at −1.14 px gives `info`; −2 px still warns; Task 21's recorded decision is updated in `docs/TEXT-INK-DIAGNOSTICS-FIX.md`.
3. **FB-44: `shadow` in interaction states.**
   - States accept only fill/opacity/radius/stroke (`crates/scene-input/src/validate/states.rs` ~8–19, message "Shadow is not supported in interaction states yet"); the render overlay has no shadow branch (`crates/server/src/jobs/states.rs` ~31–66). `docs/INTERACTION-STATES.md` ~9 says shadow has no base property, which is stale.
   - Question: add `shadow` to states? Recommended yes. Several strokes at once (border plus focus ring) is a model-wide feature and stays out (Task 46).
   - Fix: allow and validate `shadow` in states (`validate/shadow.rs`), resolve its tokens with the other state values, apply it in `jobs/states.rs`, the instance merge (`instance_paint.rs`) and the web preview (`web/src/preview/stateStyles.ts`).
   - AC: `states={"focus":{"shadow":[…]}}` is accepted; `render_frame {state:"focus"}` shows the shadow; the preview shows it on focus.
4. **FB-34: leading spaces in text are collapsed.**
   - Both measurers collapse leading/trailing spaces for every text node (`crates/text/src/wrap.rs` ~10–12, `renderer/text-wrap.ts` ~9). The report was about an instance override, but it is not instance-specific.
   - Question: (a) document the collapse and add an advisory diagnostic `TEXT_SPACE_COLLAPSED` (recommended now), or (b) add a `whiteSpace:"pre"` prop (native and TS wrap changes and a renderer version bump)?
   - AC for (a): text `"   Abc"` gets the diagnostic and the docs say to use padding; for (b): the text renders shifted, identically in Vello and the viewer.

## Out of scope

Flipping the frame `clip` default (documented in Task 43 slice B); requiring an opt-in to delete notes with a node (Task 43 reports `deletedNoteIds`; ask the user if they also want `@del x notes=delete`).

## Verify

`cargo test --offline` for the touched crates, clippy with `-D warnings`, `cargo fmt --all --check`; `npm test` and `npm run typecheck` in `web/` for items 1 and 3; `npm run test:renderer` at the root for item 4 (b); `scripts/check-public-content.sh`. Pixel changes are shown with before/after renders of synthetic fixtures.

## Outputs

One branch per item, the decision in `docs/agents/LOG.md`, updated docs and MCP texts, a status line here, and the `FB-n` → result table for the maintainer.
