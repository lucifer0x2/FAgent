# Task 44 — Agent feedback, phase 2: deeper instance overrides and large context reads

**Priority (user, 2026-11-27):** second agent-feedback phase, after [Task 43](43-feedback-phase1-small-fixes.md) slice B.
**Status:** Not assigned. Packaged 2026-11-27 from a read-only trace against `main` at `7d71753`. Items are named `FB-n` after the maintainer's private feedback log (see Task 43 "Context"); examples are synthetic.
**Depends on:** Task 43 slice B merged (same files: `instances.rs`, `instance_checks.rs`, `validate/properties.rs`, `props_catalog.rs`, `docs/COMPONENTS.md`). Item 5 is independent and may start at once.
**Owned scope:** `crates/scene-input/src/instances.rs`, `instance_paint.rs`, `validate/instance_checks.rs`, `validate/properties.rs`, `validate/layout.rs`, `props_catalog.rs`; `crates/layout/src/stack.rs`, `tree.rs`, `validation.rs`, `validation/layout.rs` and layout tests; `crates/server/src/ocd/instance_keys.rs`, `ocd/set.rs`, `ocd/props.rs` and OCD tests; `crates/server/src/reads/tree.rs`, `reads/instances.rs`, `reads/pagination.rs`, `reads/find/page.rs` (item 5); `crates/server/src/capabilities.rs`, `crates/server/skills/*.md`; server tests `instance*.rs`; `docs/COMPONENTS.md`, `docs/SERVER.md`, `docs/CONTRACT.md`.
**Workspace:** a new branch `fix/feedback-overrides` from `main` after Task 43 slice B is merged (item 5 may go on its own branch `fix/feedback-context-size`). Do not push or open a PR unless the user asks.
**Authority:** as in Task 43: code, tests, docs, commits on the branch; no deploy; temporary data roots only.

## Decision gates

Each item below has a semantic choice. Ask the user before implementing it, and record the answer in `docs/agents/LOG.md`. The recommended answer is listed first.

| Item | Question | Recommended |
| --- | --- | --- |
| 1 | Does an overridden `layout` merge per key (like `stroke`) or replace the whole object? Are `x`/`y` overridable? | Merge per key, `breakpoints` replaced whole; no `x`/`y` |
| 2 | Is `overrides.<masterRootId>` an alias for the instance's own props? | No alias; keep the Task 43 hint |
| 3 | Instance stretch: accept `fill` on the cross axis of a stretching row/column (a), or let an instance clear an inherited size, e.g. `height:"auto"` (b)? | (a) |
| 4 | `@set <inst> overrides={…}`: merge per descendant or replace? | Merge per descendant |
| 5 | Page `components` in `get_design_context` as cursor entries, or cap it with `componentsTruncated`? | Depth-limited + cursor entries |

## Items

1. **FB-12, FB-30: size and layout overrides.** `OVERRIDE_KEYS` (`instances.rs` ~13–16) has no `width`, `height`, `min*`/`max*` or `layout`, so "same button, keep its width, centre the label" cannot be expressed; `INSTANCE_KEYS` has no `layout`, so an instance cannot change the master root's padding. `merge_props` replaces `layout` whole (`instance_paint.rs` ~27). Fix: add the sizing keys and `layout` to `OVERRIDE_KEYS`, `layout` to `INSTANCE_KEYS`, and give `layout` an object merge. The root merge is written twice (`instances.rs` ~144–154 and `validate/properties.rs` ~17–31): move it into one helper first. The checks already validate the merged child in its placed scope (`instance_checks.rs` ~115–125), including fill-inside-hug.
   - AC: an override `{"width":164}` and `{"layout":{"justify":"center"}}` on a master child lays out as authored and keeps the other layout keys; an instance `layout.padding` moves its children; fill-inside-hug is still rejected under `props.overrides.<id>.width`; OCD `use#x m t1={"width":164}` compiles.
2. **FB-28: overrides through nested instances.** Master A contains an instance `b` of master B; an override `b/t` (B's text) on an instance of A is rejected (`instance_checks.rs` ~93), and even if accepted the expander passes B only its own overrides (`instances.rs` ~187–196). Fix: expansion hands each nested instance the outer overrides whose key starts with `<childInstanceId>/` (suffix as key, outer wins over the nested instance's own); validation walks the path segment by segment (each prefix an instance inside the current master, the last a descendant of the last master); turn the `.expect` on the placed lookup (~112–114) into an error. The delta closure is already transitive over nested masters (`validate/delta/affected.rs` ~120–129). OCD: accept `b/t="…"` and `b/t.text="…"` on `use` and `@set` (`ocd/instance_keys.rs` ~21).
   - AC: two- and three-level paths render; a dangling path fails with a suggestion; deleting B's text child makes A's instance fail on the next commit.
3. **FB-22: instances stretch in a stretching row.** The instance root copies the master's `width`/`height` (`instances.rs` ~146) and cannot unset them; `hug` pins the cross axis to start (`stack.rs` ~61–69); `height:"fill"` in a hug row fails "Fill inside a hug axis is ambiguous" (`validate/layout.rs` ~41–47, `crates/layout/src/validation.rs` ~128–140), even when the instance omits height but the master root says `fill`. Fix per the decision; for (a), map a cross-axis `fill` under `align:"stretch"` to an auto size with `AlignSelf::STRETCH` (`tree.rs` ~30–34) in both validators.
   - AC: in a row with `align:"stretch"`, an instance of a 64×120 master next to a 300-high frame is 300 high; a `free` hug master no longer gives 0; the same for a plain frame.
4. **FB-9 (a): `@set <inst> overrides={…}` drops the other overrides.** `@set` writes whole values for object props (`ocd/set.rs` ~28–45, `ocd/props.rs` ~102–111). Fix per the decision: compile `overrides={…}` to a merge patch per descendant; `layout={…}` and `stroke=…` keep replacing, as documented in `ocd.md`.
   - AC: `@set x overrides={"t1":{"text":"B"}}` keeps an existing `t2` override; update `ocd/tests_set.rs` ~124.
5. **FB-33: `get_design_context` too large at depth 0.** Page-1 metadata always carries `components`: the full preorder tree of every master referenced anywhere in the frame (`reads/tree.rs` ~146–157, `reads/instances.rs` ~50–90), so `depth:0` and `compact:true` do not shrink it, and `reads/pagination.rs` ~96–98 fails with `CONTEXT_ITEM_TOO_LARGE`. The error suggests "fewer viewports or a larger budget", but the default budget is already the maximum 65,536 (`schemas.rs` ~98, `tree.rs` ~227); the same message is in `reads/find/page.rs` ~67. Fix: with `depth`, list only masters referenced within that depth; page `components` as cursor entries (as `reads/tree_resources.rs` does); the error names the oversized field and its size and suggests only possible actions. Update `docs/CONTRACT.md` and `docs/SERVER.md`.
   - AC: a synthetic frame with ~1,000 nodes and ~30 masters succeeds at `depth:0, compact:true` in pages; an oversized single item names its field.

## Verify

`cargo test --offline -p fagent-layout -p fagent-scene-input -p fagent-node-memory -p fagent-server`, clippy with `-D warnings` and `cargo fmt --all --check` for those crates, `scripts/check-public-content.sh`. Render tests need the Chromium render worker; compare failures against `main`. Each item gets a test that fails on `main`.

## Outputs

Commits on the branch, updated MCP texts (`capabilities.rs`, `props_catalog.rs`, skills) and docs, a `docs/agents/LOG.md` entry with the decisions, a status line here, and the `FB-n` → result table for the maintainer.
