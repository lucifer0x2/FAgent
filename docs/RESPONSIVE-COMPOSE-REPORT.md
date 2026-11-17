# Responsive on the Compose base — 2026-11-04

## Code integration

Integrated opt-in responsive from the verified worktree into the working tree
`codex/compose-postgres` (base `f8bfc0f`). Contract at
[CONTRACT.md](CONTRACT.md#responsive-layout), requirements in [PLAN.md](../PLAN.md)
(section "Responsive opt-in").

- Child `width`/`height` accept a literal decimal `0%`–`100%` on a definite parent axis;
  min/max stay in pixels. The root is rendered per viewport, authored size is kept.
- A container has at most eight full layout alternatives, `maxWidth` is a strictly
  increasing integer in 1–100000. The first inclusive threshold is chosen by the frame
  viewport; outside the thresholds the base is used, no nested breakpoints. Every alternate
  is validated in every theme, including inactive alternates.
- Storage uses the shared parser from the layout crate; padding/gap refs in an alternate
  resolve through the existing resolver. The MCP catalog/capabilities/skills publish the
  same rules.
- Compute, HTTP scene, inspect and PNG share the same revision/viewport/theme/scene key.
  No migration is added and the authored revision is not changed when only the viewport
  changes.

Two gaps found and fixed in the finishing pass:

1. The Compose base lacked `Candidate::ancestors`, which the responsive validator calls.
   Added a bounded helper of 128 ancestors in `crates/storage/src/validate/node.rs`, within
   the existing graph/node validation responsibility. Before the fix storage failed to
   compile with E0599; after it compile/test/Clippy pass.
2. The old negative test in `crates/storage/tests/references.rs` treated `"50%"` as
   unsupported. Changed to `"100.001%"` so it still checks a value outside the allowed
   domain; production behavior was not changed to preserve the old expectation.

Responsive sources live in `crates/layout/src/{responsive,model/layout,tree,
validation/layout,util}.rs`, storage `validate/{layout,layout_rule,breakpoints}.rs`,
the catalog and server `reads/{tree,diagnostics}.rs`, capabilities/instructions/skills.
New tests: layout `responsive_tests.rs`, storage/server `tests/responsive_layout.rs`.

## Local verification evidence

Every command below was actually run on the integrated working tree. Test data uses a
temporary root; no user data or `~/.fagent` is written.

| Command (`rtk bypass` prefix omitted for readability) | Result |
| --- | --- |
| `cargo test --offline -p fagent-layout --lib -- --test-threads=2` | 34/34 pass; responsive 5, plus fixed/fill/hug/stroke/wrap/alignment regressions |
| `cargo test --offline --locked -p fagent-storage --test responsive_layout --test stack_layout --test wrap -- --test-threads=2` | 10/10 pass; all-theme alternates, placement instances, rollback/retry and wrap guards |
| `cargo test --offline --locked -p fagent-storage --test resolution --test instances --test references --test prop_catalog -- --test-threads=2` | 19/19 pass after fixing the stale negative fixture; refs/themes/instances/catalog no regression |
| `cargo test --offline --locked -p fagent-server --test responsive_layout --test wrap --test multi_viewport --test skill_layer -- --test-threads=1` | 7/7 pass; MCP resources/batch, context, inclusive boundary 600/601, inspect, HTTP scene, PNG 390×400 and revision kept |
| `cargo fmt --all -- --check` | Pass; rerun after the last test fix |
| `cargo clippy --offline --locked -p fagent-layout -p fagent-storage -p fagent-server --lib --bins -- -D warnings` | Pass for production libraries/binaries |
| `git diff --check` | Pass; rerun after the last test fix |

The server integration tests need to bind loopback. The run inside the sandbox stopped with
`PermissionDenied` at the listener; rerun with the approved escalation, all 7 tests pass.
Dead-code warnings in the shared test support are baseline; strict production Clippy has
no warnings.

`--locked` accepted Cargo.lock when `fagent-layout` moved from a dev-dependency to a
production dependency of storage. The backend and layout-step must be built from the same
source in the image; mixed-version/hot replacement is not part of the contract.

## Independent check by the coordinator

The coordinator reported real results on the same working tree:

- `cargo build --offline --locked -p fagent-layout --bin layout-step`: pass.
- `node --test --test-concurrency=1 spikes/pipeline/responsive.test.mjs spikes/pipeline/geometry.test.mjs spikes/pipeline/constraints.test.mjs spikes/pipeline/stack-align.test.mjs`:
  10/10 pass (1.607 s), after escalation for loopback. The initial sandbox run failed with
  EPERM in the before hook, an environment limit.

The golden traversal missing `effectiveTheme` was recorded as failing on a clean `main` in
the earlier responsive pass too. This integration pass did not rerun that golden and does
not use the historical baseline as evidence that the current tests pass.

## Modules and limits

Every changed Rust source/test is ≤223 lines (largest `server/src/reads/tree.rs`). New
modules: responsive parser 36, layout test 155, breakpoint validator 50, storage test 191,
server test 214 lines. Node helper 138, references test 177 lines.
`server/skills/author-frame.md` is 300 lines: a guidance document that keeps one coherent
authoring flow; not a hand-written code module. No Rust file touched exceeds 250 lines.

Not opened: full CSS/Grid, column-wrap, nested breakpoints, percentage cycles, or
mixed-version runtime. No unrelated full suite/rebuild was run. The integration worker did
not commit/push and did not change runtime/Docker/real data.

