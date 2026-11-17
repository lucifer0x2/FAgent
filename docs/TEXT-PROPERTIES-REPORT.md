# Text properties P10a/P11a/P10b — 2026-10-30

Groups 1/2 of the five-group property batch. Run on the uncommitted working tree
`main`@`6ccf191`, **with the paint lane already integrated** (corners/shadow/gradient).
Pipeline version stays `scene-3`/`web-4` (aggregate not released, no bump). Environment:
macOS, Node 24.21.0, Playwright 1.64.1, Chromium 156.0.8078.4, rustc 1.99.1; fonts pinned
by hash: `renderer/fonts/NotoSans.ttf`, `fixtures/fonts/Lato-Regular.ttf`.

## Implemented per AC

**P10a — metrics, width, wrap, whitespace**

- Baseline = `fontBoundingBoxAscent + floor((lineHeight − ascent − descent)/2)`, i.e.
  half-leading from font metrics as in CSS; a lineHeight smaller than the font (negative
  leading) and odd values both match the DOM marker.
- Reported width and the fit check use the same `ceil64` grid; text re-measured at its own
  width does not break the line (`Prefer` hug at W stays 1 line, W−1/64 becomes 2 lines).
- `textWrap: wrap|nowrap`; LF always breaks; horizontal whitespace collapses (equivalent to
  `white-space: pre-line`). NBSP U+00A0, narrow NBSP U+202F, word joiner U+2060 are not
  break points; long words break as an emergency by grapheme (never splitting
  mark/ZWJ/flag clusters). Nowrap overflows horizontally, start-aligned like Chrome.
- Modules: `renderer/text-wrap.ts`, `renderer/measurement.ts` (coordination),
  `crates/layout/src/model/text.rs` (wire contract), `crates/layout/src/text_lines.rs`
  (validate/align/decoration/diagnostics, no re-measuring, does not modify source).

**P11a — missing glyphs**

- `renderer/glyph-coverage.ts` reads the SFNT cmap format 4/12 from the pinned bytes (not
  inferred from "font loaded successfully"). NFC and NFD accented text raise no false
  warnings.
- Missing mapping → `MISSING_GLYPH node=… font=…: cmap has no mapping for U+…` (at most 8
  code points per node, at most 20 warnings in total + `GLYPH_DIAGNOSTICS_LIMIT`).
- WOFF/WOFF2/unknown container/broken cmap → no coverage → `GLYPH_CHECK_UNAVAILABLE`, no
  made-up pass. Geometry is identical with and without coverage.

**P10b — tracking, align, decoration, transform**

- `letterSpacing`: signed px, finite, −1000..1000; goes into the measure request/cache key;
  the scene carries `letterSpacing`. Tracking changes wrap points at the boundary and
  causes proper cache misses.
- `textAlign: left|center|right` computed after measuring in `text_lines::align` (center
  rounds down to 1/64 like Chrome).
- `textDecoration: none|underline|line-through` → scene `decoration{line,offset,thickness}`;
  drawn continuous, no skip-ink (equivalent to CSS `text-decoration-skip-ink:none`, noted
  in the props guide and capability).
- `textTransform: none|uppercase|lowercase|capitalize` (`renderer/text-transform.ts`)
  applies only to the displayed/measured string; the original `props.text` is kept in
  context, snapshot, bundle (`Straße`→`STRASSE`, `ﬁ`→`FI`, `o'neil`→`O'neil`).
- Consumers use the same scene line origins: canvas/PNG `renderer/paint-text.ts`, DOM
  preview `web/src/preview/textContent.ts`, SVG production `crates/server/src/svg/text.rs`
  (embedded font, `letter-spacing`, decoration as `<rect>`).
- Storage: `crates/storage/src/validate/text_props.rs` for nodes and text styles, atomic
  rollback on a wrong value or wrong theme token; preserved across style/theme/instance/
  snapshot/bundle.
- Extent: `renderer/scene-tree.ts` unions `line.inkBounds` (measured from
  `actualBoundingBox*` with tracking) into the text extent, so a group's opacity layer does
  not clip ink with large negative tracking/overhang.

## Commands run (on the current tree, final results)

| Command | Result |
| --- | --- |
| `cargo test -p fagent-layout --lib` | 27 pass |
| `npm run check:renderer` | tsc pass (exit 0) |
| `node --test renderer/glyph-coverage.test.mjs renderer/text-extent.test.mjs` | 7 pass (3 + 4) |
| `npm run build:renderer` + `cargo build -p fagent-layout --bin layout-step` | build OK |
| `node --test --test-concurrency=1 spikes/pipeline/text-measure.test.mjs` | 5 pass |
| `node --test --test-concurrency=1 spikes/pipeline/text-pixels.test.mjs` | 4 pass |
| `cargo test -p fagent-storage --test text_props` | 2 pass |
| `cargo test -p fagent-server --test text_rendering` (calls `spikes/pipeline/text-svg-offline.mjs`) | 1 pass |
| `cargo test -p fagent-server --test font_face_context` | 2 pass |
| `rustfmt --check` on the text group's Rust files (edition per crate) | clean |

The last two pipeline suites ran together in one command after rebuilding the renderer: 9/9
pass. Server tests use a temporary data root + `127.0.0.1:0`; they do not touch
`~/.fagent`, port 3847, containers or volumes. No Chromium/layout-step/server process is
left after the run.

**text-measure** (compared with the DOM in the same Chromium, no tolerance): 2 fonts × 6
sizes × 40 strings × tracking {−0.5, 0, 1.25} = 1,440 combinations, width/baseline exactly
equal to the DOM and re-measuring at its own width still gives 1 line; baseline for 14
font×(size,lineHeight) pairs including 11/13, 15/15, 13/16; NBSP/U+202F/U+2060/grapheme;
tracking {−1,0,2} wrap boundary + cache hit; coverage; transform preserves source.

## Pixels (narrow tolerance)

`spikes/pipeline/text-pixels.test.mjs`, comparing canvas with the CSS oracle and the
production DOM preview:

- Matrix 1: 2 fonts × {14/20, 24/32} × tracking {−0.5,0,1.25} × {left, center, right,
  center+underline, right+line-through} = 60 CSS + 60 preview.
  - Preview: **60/60 match with 0 pixels**.
  - CSS line-through: **12/12 match with 0 pixels**; no tolerance for strike.
  - CSS underline: **12/12 differ by 5–15 pixels, maxDelta 1 channel, `unexpected` 0**
    (Noto: 6/5/6 at 14px, 12/15/15 at 24px; Lato: 6/7/7 and 12/12/12). Tolerance applies
    only to pixels inside the underline rectangle (excluding the two end pixels), where
    canvas is solid ink color and differs by ≤1; the ends and all other pixels must be
    exact.
- Matrix 2: 8 sizes × 2 lineHeight ratios × {underline, line-through} + a 3-line
  uppercase/center/tracking/strike paragraph with a line-list comparison: **66
  comparisons, 0 differing pixels**.
- Opacity layer: group opacity 0.5 containing text with `letterSpacing −30` (ink shifted
  left to x≈89, the old fallback extent only reached x=218) + a rect to force the layer.
  806 ink pixels lie outside the old extent; |darkness(0.5) − darkness(1)/2| at most
  **1.5** (two 8-bit roundings). The negative control (removing `inkBounds` from the scene)
  gives **127.5**, i.e. the layer is clipped — the test can tell them apart.

## Image evidence

`docs/text-properties-evidence/canvas.png` and `production-svg.png` (400×240) are
regenerated by `text_rendering` on the current tree; the test asserts 0 differing pixels
between canvas and the production SVG opened offline (no HTTP requests, font data URL), and
that `<text>` coordinates equal the scene. The two files are byte-identical (SHA-1
`1540eca…`).

Opened and viewed: the Lato 24/32 center, capitalize, tracking −0.5, underline line
"Straße Copy Ångström / Hello-World O'neil" — two centered lines, underline continuous and
passing through the `p` descender (no skip-ink, per contract). The Noto 14/20 right block,
tracking 1.25, line-through "Copy Ångström / second line ñ" — right edge aligned,
diacritics correct, strike at mid x-height. The two `customers` nowrap lines in a box 20
wide, center-aligned, still start at x=0 and overflow right like Chrome. No difference seen
between the two images. Worth knowing: the underline/strike ends include the tracking after
the last character (like CSS).

## Limits that must stay explicit

- cmap proves only **scalar mapping**; it does not prove complex shaping, emoji ZWJ/
  skin-tone, variation sequences, or correct ligatures/clusters.
- WOFF/WOFF2/unreadable formats → `GLYPH_CHECK_UNAVAILABLE`, coverage not checked.
- Missing glyphs are **advisory** only; there is no pinned fallback chain, and rendering
  does not fail.
- Pixels are guaranteed only on the pinned runtime/font above (macOS + Chromium 156); **no
  cross-OS pixel promise**, Docker/Linux, Safari/Firefox not measured.
- Tracking in px only; no rich runs, RTL, ellipsis/maxLines, vertical align, or variable
  axes.
- SVG keeps text + embedded font, no outlines (`SVG_TEXT_NOT_OUTLINED`); display depends on
  the viewer supporting `@font-face` data URLs.
- CSS underline has a 1-channel difference in the glyph/underline overlap area, as in the
  numbers above.

**Opacity of self-overlapping text (R2).** Text with decoration, negative `letterSpacing`,
or lines whose ink touches (ink rect grown by 1 unit) under the node's own opacity is
composited once as in CSS: Canvas/PNG counts two paint units to force an opacity layer
(`renderer/text-overlap.ts`, extent includes the decoration rects); SVG wraps `<text>`/
`<rect>` in a single `<g opacity>` (`svg/text.rs::self_overlaps`; negative tracking within
one `<text>` is already composited once). Remaining: self-overlapping glyphs at tracking
≥ 0 (script fonts, tight kerning) are not detected, so Canvas still composites twice; when
`MAX_LAYER_PIXELS` is exceeded, the group falls back to folding alpha into each paint
(`opacityLayerFallbacks`) and the overlap area is darker than CSS.

## Changes in this finishing session

- `renderer/text-extent.test.mjs`: the old test could not run (Node does not resolve the
  extensionless imports of `scene-tree.ts`, `ERR_MODULE_NOT_FOUND`); it now bundles with
  esbuild in memory and has 4 cases (left ink, right/bottom ink, fallback without
  inkBounds, group).
- `spikes/pipeline/text-pixels.test.mjs`: prints the matrix 2 comparison count; adds an
  opacity-layer test through the real pipeline (with the negative control run by hand).
- `rustfmt` for `text_lines.rs`, `svg/text.rs`, `tests/text_rendering.rs`,
  `tests/text_props.rs` (formatting only).
- No shared files with the paint lane were modified; the inkBounds union in `scene-tree.ts`
  was already present.

Size: every text file ≤155 lines. `cargo fmt --all --check` still reports differences in
paint/shared files (engine, svg/mod, svg/elements, validate/*, paint_tests…), not part of
this group; `git diff --check` reports a trailing blank line in
`crates/storage/src/validate/stroke.rs` (paint lane).
