import assert from "node:assert/strict";
import test from "node:test";
import {
  FRAME_GAP,
  FRAME_LABEL,
  fitFrameBoard,
  packFrames,
  type FramePlacement,
  type FrameSize,
} from "../src/scene/frameLayout.ts";
import { fitPageBoard, type BoardSection } from "../src/scene/pageBoard.ts";

type Rect = { x: number; y: number; width: number; height: number };
const EPS = 1e-6;
const overlaps = (a: Rect, b: Rect) =>
  a.x < b.x + b.width - EPS && b.x < a.x + a.width - EPS &&
  a.y < b.y + b.height - EPS && b.y < a.y + a.height - EPS;
const frame = (id: string, width: number, height: number): FrameSize => ({ id, width, height });
// A label band reaches up to whatever sits over the frame, or the board top (y = 0).
const labelBand = (placed: FramePlacement): Rect => {
  const height = Math.min(placed.labelSpace ?? FRAME_LABEL, placed.y);
  return { x: placed.x, y: placed.y - height, width: placed.width, height };
};
const wasted = (layout: { width: number; height: number }, rects: Rect[]) =>
  1 - rects.reduce((sum, r) => sum + r.width * r.height, 0) / (layout.width * layout.height);

/**
 * Reading order: a later item never starts both above and left of an earlier one, so the
 * authored order reads left to right, top to bottom (a new column or a new row).
 */
function assertReadingOrder(items: { x: number; y: number }[], label: string) {
  for (const [i, a] of items.entries())
    for (const b of items.slice(i + 1))
      assert.ok(!(b.x < a.x - EPS && b.y < a.y - EPS), `${label}: ${JSON.stringify([a, b])}`);
}

/** Frames ≥ gap apart, each label band (up to what sits over it) clear of every other frame. */
function assertClear(placed: FramePlacement[], label: string) {
  for (const a of placed) {
    assert.ok((a.labelSpace ?? 0) >= FRAME_LABEL - EPS, `${label}: ${a.id} label room`);
    for (const b of placed) {
      if (a === b) continue;
      const grown = { x: a.x - FRAME_GAP + EPS, y: a.y - FRAME_GAP + EPS, width: a.width + 2 * FRAME_GAP - 2 * EPS, height: a.height + 2 * FRAME_GAP - 2 * EPS };
      assert.ok(!overlaps(grown, b), `${label}: ${a.id}/${b.id} closer than the gap`);
      assert.ok(!overlaps(labelBand(a), b), `${label}: ${b.id} covers ${a.id}'s label`);
    }
  }
}

// One long mobile page among phone screens: a shelf row is as tall as the long page, so the
// old packer left the phones in one band beside it (25.0M board units², 80% empty).
const tall = [frame("long", 390, 6000), ...Array.from({ length: 8 }, (_, i) => frame(`p${i}`, 390, 844))];
const mixed = Array.from({ length: 40 }, (_, i) =>
  frame(`m${i}`, [390, 1440, 768, 1920, 360][i % 5], [844, 900, 1024, 5200, 120][(i * 7) % 5]));

test("short frames stack beside a tall one: less board and less empty area than shelf rows", () => {
  const layout = fitFrameBoard(tall);
  assertClear(layout.frames, "tall");
  // Old shelf rows at a 1440×900 pane: 4022 × 6223 (25.0M), 80.1% empty.
  assert.ok(layout.width * layout.height < 9e6, `${layout.width}×${layout.height}`);
  assert.ok(wasted(layout, layout.frames) < 0.45, String(wasted(layout, layout.frames)));
  // No phone sits below the long page: they fill the columns next to it.
  const long = layout.frames[0];
  for (const placed of layout.frames.slice(1))
    assert.ok(placed.y + placed.height <= long.y + long.height + EPS, placed.id);
});

test("packing keeps labels clear, reads in authored order and starts top-left", () => {
  for (const aspect of [0.5, 1, 1.6, 3])
    for (const input of [tall, mixed, mixed.slice(0, 7), [...tall].reverse()]) {
      const layout = packFrames(input, aspect);
      const label = `${input.length}@${aspect}`;
      assert.deepEqual(layout.frames.map((f) => f.id), input.map((f) => f.id));
      assert.deepEqual([layout.frames[0].x, layout.frames[0].y], [0, FRAME_LABEL]);
      assertClear(layout.frames, label);
      assertReadingOrder(layout.frames, label);
      for (const placed of layout.frames) {
        assert.ok(placed.x + placed.width <= layout.width + EPS, label);
        assert.ok(placed.y + placed.height <= layout.height + EPS, label);
      }
    }
});

test("the same frames give the same board, without mutating the input", () => {
  const input = structuredClone(mixed);
  const first = fitFrameBoard(input);
  assert.deepEqual(input, mixed);
  assert.deepEqual(fitFrameBoard(structuredClone(mixed)), first);
  // No viewport parameter: a pane or drawer change cannot repack the board.
  assert.equal(fitFrameBoard.length, 1);
});

// The All pages case from the report: a tall app page, then small pages. With shelf rows
// at a 900×900 pane, the last page started under the tall one (y = 5282, 66.8% empty).
const pageFrames = [
  frame("home", 390, 4200), frame("shop", 390, 3600), frame("cart", 390, 2400),
  frame("login", 390, 844), frame("signup", 390, 844),
  frame("dash", 1440, 900),
  frame("modal", 600, 400), frame("toast", 360, 120),
  frame("tab0", 768, 1024),
  frame("icon", 512, 512), frame("icon2", 512, 512),
];
const pages: BoardSection[] = [
  { id: "app", name: "App", frameIds: ["home", "shop", "cart"] },
  { id: "auth", name: "Auth", frameIds: ["login", "signup"] },
  { id: "web", name: "Web", frameIds: ["dash"] },
  { id: "overlays", name: "Overlays", frameIds: ["modal", "toast"] },
  { id: "tablet", name: "Tablet", frameIds: ["tab0"] },
  { id: "icons", name: "Icons", frameIds: ["icon", "icon2"] },
];

test("All pages: small pages fill beside a tall page instead of a row under it", () => {
  const board = fitPageBoard(pageFrames, pages);
  const app = board.sections[0];
  for (const section of board.sections.slice(1))
    assert.ok(section.focus.y < app.y + app.height, `${section.id} starts below the tall page`);
  assert.ok(board.height <= app.y + app.height + EPS);
  // Old shelf rows: 36.0M–37.9M board units² and 67–68% empty, by pane.
  const area = board.width * board.height;
  assert.ok(area < 24e6, String(area));
  assert.ok(wasted(board, board.sections.map((s) => s.focus)) < 0.45);
  assertReadingOrder(board.sections, "pages");
  for (const [i, a] of board.sections.entries())
    for (const b of board.sections.slice(i + 1)) assert.ok(!overlaps(a.focus, b.focus));
  // Labels and titles: no frame covers a frame label or another page's title room.
  assertClear(board.frames, "board");
  for (const placed of board.frames) {
    const own = board.sections.find((s) => pages[s.index].frameIds.includes(placed.id))!;
    assert.ok(labelBand(placed).y >= own.y - EPS, `${placed.id} label leaves its page`);
    for (const other of board.sections)
      if (other !== own) assert.ok(!overlaps(placed, other.focus), `${placed.id} in ${other.id}`);
  }
  assert.deepEqual(fitPageBoard(structuredClone(pageFrames), structuredClone(pages)), board);
});

test("200 frames on one page and over 30 pages pack within a frame budget", () => {
  const many = Array.from({ length: 200 }, (_, i) =>
    frame(`f${i}`, [390, 1440, 768, 1920, 390][i % 5], [844, 900, 1024, 1080, 3200][(i * 7) % 5]));
  const sections = Array.from({ length: 30 }, (_, p) => ({
    id: `s${p}`, name: `S${p}`, frameIds: many.filter((_, i) => i % 30 === p).map((f) => f.id),
  }));
  let start = performance.now();
  const page = fitFrameBoard(many);
  const single = performance.now() - start;
  start = performance.now();
  const board = fitPageBoard(many, sections);
  const all = performance.now() - start;
  assert.equal(page.frames.length, 200);
  assert.equal(board.frames.length, 200);
  assertReadingOrder(page.frames, "200");
  // Computed once per content change (never per resize or zoom); generous for loaded CI.
  assert.ok(single < 500 && all < 500, `${single.toFixed(1)} ms / ${all.toFixed(1)} ms`);
});
