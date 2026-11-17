import assert from "node:assert/strict";
import test from "node:test";
import type { FrameSize } from "../src/scene/frameLayout.ts";
import { fitPageBoard, type BoardSection } from "../src/scene/pageBoard.ts";

type Rect = { x: number; y: number; width: number; height: number };
const EPS = 1e-6;
const overlaps = (a: Rect, b: Rect) =>
  a.x < b.x + b.width - EPS && b.x < a.x + a.width - EPS &&
  a.y < b.y + b.height - EPS && b.y < a.y + a.height - EPS;
const inside = (inner: Rect, outer: Rect) =>
  inner.x >= outer.x - EPS && inner.y >= outer.y - EPS &&
  inner.x + inner.width <= outer.x + outer.width + EPS &&
  inner.y + inner.height <= outer.y + outer.height + EPS;

const frame = (id: string, width: number, height: number): FrameSize => ({ id, width, height });
// Mixed pages: phone screens, a wide desktop page, one huge canvas and an empty page.
const frames = [
  ...Array.from({ length: 7 }, (_, i) => frame(`m${i}`, 390, 844)),
  frame("d0", 1440, 900), frame("d1", 1440, 2400),
  frame("huge", 6000, 4000),
  frame("t0", 768, 1024), frame("t1", 768, 1024),
];
const sections: BoardSection[] = [
  { id: "mobile", name: "Mobile", frameIds: frames.slice(0, 7).map((f) => f.id) },
  { id: "desktop", name: "Desktop", frameIds: ["d0", "d1"] },
  { id: "empty", name: "Empty", frameIds: [] },
  { id: "canvas", name: "Canvas", frameIds: ["huge"] },
  { id: "tablet", name: "Tablet", frameIds: ["t0", "t1", "missing"] },
];
const board = fitPageBoard(frames, sections);

{
  test(`pages keep their order and do not overlap, title rooms included`, () => {
    assert.deepEqual(board.sections.map((s) => s.id), sections.map((s) => s.id));
    assert.deepEqual(board.sections.map((s) => s.index), [0, 1, 2, 3, 4]);
    for (const [i, a] of board.sections.entries())
      for (const b of board.sections.slice(i + 1))
        assert.ok(!overlaps(a.focus, b.focus), `${a.id} and ${b.id} overlap`);
  });

  test(`every frame lies inside its own page, in authored order`, () => {
    const at = new Map(board.frames.map((placed) => [placed.id, placed]));
    // A frame ID the snapshot does not hold is dropped; every known frame is placed once.
    assert.equal(board.frames.length, frames.length);
    assert.equal(at.has("missing"), false);
    for (const section of board.sections) {
      const own = sections[section.index].frameIds.filter((id) => at.has(id));
      assert.equal(section.frames, own.length);
      for (const id of own) {
        const placed = at.get(id)!;
        assert.equal(placed.width, frames.find((f) => f.id === id)!.width);
        assert.ok(inside(placed, section), `${id} outside ${section.id}`);
        for (const other of board.sections)
          if (other !== section) assert.ok(!overlaps(placed, other.focus), `${id} in ${other.id}`);
      }
    }
  });

  test(`focus covers the page and the title room above it, within the board`, () => {
    for (const section of board.sections) {
      const { focus } = section;
      assert.ok(inside(section, focus));
      assert.equal(focus.x, section.x);
      assert.equal(focus.width, section.width);
      assert.ok(focus.y < section.y, `${section.id} has no title room`);
      assert.ok(Math.abs(focus.y + focus.height - (section.y + section.height)) < EPS);
      assert.ok(inside(focus, { x: 0, y: 0, width: board.width, height: board.height }));
    }
  });

  test(`an empty page still gets a block`, () => {
    const empty = board.sections.find((s) => s.id === "empty")!;
    assert.equal(empty.frames, 0);
    assert.ok(empty.width >= 320 && empty.height >= 120, JSON.stringify(empty));
  });

  test(`the same input gives the same layout`, () => {
    assert.deepEqual(fitPageBoard(structuredClone(frames), structuredClone(sections)), board);
  });
}

test("a small board that fits at 100% keeps its title rooms on the board", () => {
  const small = [frame("a", 200, 120), frame("b", 160, 90)];
  const board = fitPageBoard(small, [
    { id: "p1", name: "One", frameIds: ["a"] },
    { id: "p2", name: "Two", frameIds: ["b"] },
  ]);
  const bounds = { x: 0, y: 0, width: board.width, height: board.height };
  assert.ok(!overlaps(board.sections[0].focus, board.sections[1].focus));
  for (const section of board.sections) {
    assert.ok(section.focus.y < section.y);
    assert.ok(inside(section.focus, bounds), JSON.stringify(section));
  }
});

test("no pages and pages of only unknown frames stay well-formed", () => {
  assert.deepEqual(fitPageBoard(frames, []), {
    frames: [], sections: [], width: 0, height: 0,
  });
  const ghost = fitPageBoard([], [{ id: "p", name: "P", frameIds: ["gone"] }]);
  assert.equal(ghost.frames.length, 0);
  assert.equal(ghost.sections[0].frames, 0);
  assert.ok(ghost.sections[0].width > 0 && ghost.sections[0].height > 0);
});

// Canvas panes (CSS px): a 1440×900 window with the drawers closed, a narrow 900×900 one
// (drawers open), a wide 2560×1300 one. Fit leaves 96×120 px for the chrome, as the viewer.
const panes = [
  { width: 1440, height: 900 },
  { width: 900, height: 900 },
  { width: 2560, height: 1300 },
];

test("the layout takes no viewport: panes and drawers only move the camera", () => {
  // The API has no size parameter, so a pane or drawer change cannot repack the board.
  assert.equal(fitPageBoard.length, 2);
  for (const pane of panes) {
    const scale = Math.min((pane.width - 96) / board.width, (pane.height - 120) / board.height, 1);
    // Every page title (36 px room when the page alone is fitted, 24 px tab) stays readable
    // when the whole board is fitted into any of these panes.
    for (const section of board.sections) {
      const room = (section.y - section.focus.y) * scale;
      assert.ok(room > 0, `${section.id} at ${pane.width}×${pane.height}`);
    }
    // Board title room above the top row reads at the whole board's Fit.
    const top = Math.min(...board.sections.map((s) => s.y));
    assert.ok(top * scale >= 24, `title room ${top * scale} px at ${pane.width}×${pane.height}`);
  }
});
