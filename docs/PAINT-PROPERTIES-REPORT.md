# Group 5 report — paint (P17/P04b/P09b/P09a) — 2026-10-30

Scope: edge/corner shorthand, per-corner radius, outer/inset shadow, linear/radial
gradient on the current Chromium pipeline. The pipeline is still the unreleased aggregate
`scene-3`/`web-4` (no version bump). No blur, vector, `%`, rich text, or variable axes.

## Implemented semantics

- **radius**: a number or 1–4 CSS values `[tl, tr, br, bl]`, each accepting a dimension
  token. Authored form is kept as written; in the scene a scalar number keeps its old form
  and an array always becomes 4 values. Overlapping corners shrink the CSS way: a single
  factor `f = min(side / sum of the two adjacent radii)` on the border box, applied to
  every corner (canvas `roundRect`, `renderer/corners.ts`, SVG `svg/geometry.rs`).
- **stroke**: width/color take 1–4 values as before. The stroke ring uses the node's
  reduced radii: inside = CSS border (inner radius = outer − adjacent width, ≥ 0, **not
  reduced a second time**); center/outside grow by `width·f` from the reduced fill shape.
- **fill gradient** (box only, not text): `{type:"linear", angle?:180, stops}` or
  `{type:"radial", center?:[.5,.5], radius?: number | [rx, ry], stops}`; 2..16 stops as
  `[position 0..1 non-decreasing, color]`, hard stops keep their order; stop colors accept
  color tokens; a fill style may be a gradient. Premultiplied interpolation as in CSS on
  canvas, preview and SVG. Fill animation needs a solid resting fill: a node with a
  gradient fill (literal or via a fill style) that declares a `fill` keyframe is rejected
  at `props.animations[i].keyframes[j].fill`, because the HTML preview paints the gradient
  with `background-image`, which hides the running color. A gradient only inside a state
  is still valid (state always wins over animation fill). Preview warns if legacy data
  slips through.
- **shadow**: 0..8 layers `{x,y,blur,spread,color,inset}`, first layer on top; x/y/spread
  accept number tokens, blur a dimension token, color a color token. Outer + inset; a
  translucent inner fill cuts out the outer shadow as in CSS.
- **states**: fill (including gradient), opacity, radius (1–4), stroke. **Shadow inside a
  state is still rejected**, error `props.states.<state>.shadow` says so explicitly: not
  supported yet, put the shadow on the resting node and change fill/opacity/radius/stroke
  per state.
- **Instance**: a `fill` override (including gradient) is validated like the base prop;
  radius/shadow/stroke come from the master; a shadow override is rejected at
  `props.overrides.<id>.shadow`.
- **SVG production**: radii → `<path>` (arcs, CSS factor), gradient →
  `<linearGradient>`/`<radialGradient>` with expanded stops (see A). Shadow and stroke are
  not drawn, and each file reports `SVG_SHADOW_NOT_EXPORTED` / `SVG_STROKE_NOT_EXPORTED`;
  nothing is dropped silently.

## Real bugs fixed in this batch

1. Storage: a `fill` gradient inside `states` was **not structurally validated** (the
   object was skipped like a ref) → decreasing stops slipped into the scene. Now validated
   like the base prop, error `props.states.hover.fill.stops[1]`.
2. Storage: a wrong literal color in a shadow reported field `color` instead of
   `shadow[i].color`.
3. Canvas stroke: the inner radius was computed from the unreduced radius and then reduced
   again by `roundRect`; the color-split line was clamped per axis and so was off at the
   corners with large radii; a multi-color ring painted the first color as background and
   then covered it with other colors (red border bleeding at AA edges). See B.
4. Preview: the stroke overlay used the unreduced radius for center/outside.
5. SVG: alpha gradients interpolated in straight alpha. See A.
6. Clippy `never_loop` in `validate/{paint,shadow}.rs`.

## Review point A — SVG alpha gradients

Chromium experiment (hand-written SVG, stop-color + stop-opacity, compared with
`linear-gradient`): maximum difference **64** for red → `#0000` and red → `#00f0`, **20**
for `#ff000080 → #0000ffcc` (mid-ramp: CSS `255,128,128`, SVG `191,128,128`). The
`color-interpolation` flag does not change this. Fix: `crates/server/src/svg/stops.rs`
expands stops like `renderer/fill.ts` — an alpha-0 stop takes the neighbor's color on both
sides; a segment between two different (non-0) alphas is split into 64 steps along the
premultiplied curve; output is 6-digit hex + `stop-opacity` (readable in SVG 1.1).
Production pixel check (`tests/paint_svg.rs` → `spikes/pipeline/paint-svg-offline.mjs`,
offline, no network requests): SVG vs CSS **1 px / max 8** (scale 1), **6 px / max 8**
(scale 2), covering red→`#0000`, red→`#00f0`, differing alphas, radial alpha, default
ellipse.

## Review point B — large asymmetric radii + uneven widths

Verified with real Chromium pixels (CSS border), not only shapes:
- Outside: CSS factor on the border box, correct on canvas (`roundRect`) and SVG `<path>`.
- Inside: Chromium uses exactly `outer − adjacent width` (≥ 0) and **does not reduce a
  second time** when the inner radius overflows the inner box (sample `[100,20,80,5]`,
  width `[1,1,20,1]`: the tl arc cuts the bottom edge). Reducing twice (the old way) was
  off by 952 px / max 255; per-axis 658 px; no reduction 143 px / max 64 (the rest is AA).
  Canvas now builds the inner region by clipping the ellipse corner pieces
  (`renderer/stroke-ring.ts`), not through `roundRect`.
- Color split line: from the outer corner through the inner corner to the chord of the
  inner arc (like Chromium's edge clip), draw order: translucent inside first, then
  top/bottom before right/left; same-color seams are pushed ~1 device px (Chromium clips
  without AA); square corners of different colors: the edge drawn first covers the seam
  (`renderer/stroke-sides.ts`).
- Single-color border, uniform width, every corner > w/2: Chromium draws it as a
  center-line stroke; canvas does the same (1px r12: 148 px / max 88 → 0 px).
- Preview: the inside overlay emits the authored radius so CSS reduces it itself;
  center/outside emit the reduced radius. Preview vs CSS: at most 3 px / max 9 over 60
  comparisons.

## Commands and results (run)

| Command | Result |
| --- | --- |
| `cargo test -p fagent-storage --test paint_props --test paint_props_reuse --test stroke_props --test resolution --test bundle_roundtrip --test states --test instances` | 3+2+6+9+3+3+4 pass |
| `cargo test -p fagent-layout --lib` | 29 pass (6 paint/stroke) |
| `cargo test -p fagent-server --lib` | 42 pass (including `svg::stops`) |
| `cargo test -p fagent-server --test states_render --test stroke_context --test export_svg --test states_context --test paint_svg --test instances_states --test skill_layer --test tools_budget` | 1+2+2+3+1+2+2+1 pass |
| `npm run check:renderer`, `npm --prefix web run typecheck` | clean |
| `npm run test:renderer` | 10/10 |
| `npm --prefix web test` | 24/24 |
| `npm run build:renderer` + `cargo build -p fagent-layout --bin layout-step`; `node --test --test-concurrency=1 spikes/pipeline/{paint-properties,stroke,stroke-opacity,opacity}.test.mjs` | 21/21 |
| `cargo clippy -p fagent-storage --all-targets`, `-p fagent-layout --all-targets`, `-p fagent-server --lib --test paint_svg` (`-D warnings`) | clean |

`cargo clippy -p fagent-server --all-targets -D warnings` still has errors outside the
paint scope: `tests/states_render.rs:67` (`chunks_exact`), dead code in
`tests/support/mod.rs`.

## Pixel counts (pixels with a channel difference > 2 / max difference), canvas vs Chromium CSS

| Case | scale 1 | scale 2 |
| --- | --- | --- |
| G1–G7, G9 gradient | 0 / ≤1 | ≤1 / ≤8 |
| G8 hard stop 60° | 0 / 1 | 20 / 42 (AA at rounded corner, see below) |
| S1–S7, S9, S10 shadow | 0 / ≤2 | 0 / ≤2 |
| S8 translucent inner fill + shadow | 11 / 8 | 13 / 7 |
| stroke 1px r12, width `[1,2,3,4]`, B-uniform | 0 / 0 | ≤13 / ≤7 |
| B-oversized, B-overflow, B-sides | 135–175 / ≤64 | 262–316 / ≤66 |
| multi-color seam (sides, keycap, translucent, square) | 9–76 / ≤72 | 14–153 / ≤109 |
| group opacity (clip/no clip), edge-crop | 0 / ≤1 | ≤2 / ≤3 |

Test budgets (`spikes/pipeline/paint-fixtures.mjs`, multiplied by scale): `edge` 12 px /
max 48, `seam` 80 px / 110, `arc` 190 px / 96. For AA only: a geometry error ≥ 1 px shows
up as a difference near 255. `edge` baseline: a solid 120×220 r12 box without gradient is
already off by 19 px / max 42 at scale 2 near the bottom corner. G8 uses 60°: at 45° the
pixel center lies exactly on the hard stop, and CSS and canvas break the tie differently
(69 px / 255 at scale 2). `arc`: large asymmetric arcs, canvas and Chromium choose
different AA paths (clip vs fill vs stroke); preview vs canvas center/outside ≤ 347 px /
max 86 at scale 2, ≤ 7 px / max 8 at scale 1.

## Group opacity and crop at the export edge

- The opacity layer does not clip the shadow: the test checks a pixel 6 px below the group
  bottom (inside the blur, outside the group box) still has shadow when `clip:false`, white
  when `clip:true`; matches CSS ≤ 1.
- The export viewport is still the layout bounds: a node at the bottom-right corner with
  shadow `x6 y8 blur16` — canvas 480×300·scale, the inner shadow is still drawn, and the
  part beyond the edge is cut exactly like CSS seen through a 480×300 window (≤ 2 px / max
  3). This is a **viewport crop**, a published limit, distinct from the opacity-layer clip
  bug.

## Published limits and open risks

- SVG: shadow/stroke are not drawn, with a per-file diagnostic; export PNG if needed.
- Not supported: blur, conic gradient, gradient/shadow on text, shadow in a state.
- Preview: a center stroke with odd width places the overlay at half a CSS px, and Chromium
  snaps (1140 px / max 208 at scale 1, width `[3,9,5,13]`) — a pre-existing fractional-DOM
  geometry limit; tests use even widths for center.
- Single-color stroke as a center-line stroke: some straight-edge pixels fall short by
  ≤ 3/255 alpha (Skia stroker); `stroke.test.mjs` compares premultiplied within ±3 rather
  than exact equality.
- Inset shadow + inside stroke when the inner radius overflows the box: the clip still goes
  through `roundRect` (reduced twice); no pixel test for this combination yet.
- Canvas fill of a 76 px arc differs from CSS by AA at scale 2 (49 px / max 38 in the SVG
  case).
- The root `tsconfig.json` adds `allowImportingTsExtensions` so the `stroke.ts` chain can
  be imported when web tests run TS under Node.

## Files

New: `renderer/stroke-ring.ts`, `renderer/stroke-sides.ts`, `crates/server/src/svg/stops.rs`,
`crates/server/tests/paint_svg.rs`, `crates/storage/tests/paint_props_reuse.rs`,
`spikes/pipeline/paint-svg-offline.mjs`, this report. Modified: `renderer/{corners,stroke}.ts`,
`web/src/preview/{paintStyle,sceneDom,stateStyles}.ts`, `crates/storage/src/validate/
{paint,shadow,states,quad}.rs`, `crates/storage/tests/paint_props.rs`,
`crates/layout/src/{paint_tests.rs,model/fill.rs,model/shadow.rs}` (rustfmt),
`crates/server/src/svg/{paint,geometry,mod}.rs`, `crates/server/src/jobs/states.rs` (rustfmt),
`crates/server/src/props_doc.rs`, `crates/server/skills/{author,implement}-frame.md`,
`docs/SERVER.md`, `spikes/pipeline/paint-fixtures.mjs`, `spikes/pipeline/paint-properties.test.mjs`,
`spikes/pipeline/stroke.test.mjs`, `tsconfig.json`.
