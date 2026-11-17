import assert from "node:assert/strict";
import test from "node:test";
import { pinchStep, pointerPair } from "../src/hooks/useCanvasGestures.ts";
import {
  panDragStep,
  PointerGesture,
  startPanDrag,
  type CanvasGestureHandlers,
} from "../src/hooks/canvasPointers.ts";
import {
  clampScale,
  panTransform,
  wheelZoomFactor,
  zoomTransform,
  type Point,
  type Transform,
} from "../src/hooks/useCanvasTransform.ts";
import {
  clampPanelWidth,
  isPanelWidth,
  PANEL_MAX_WIDTH,
  PANEL_MIN_WIDTH,
} from "../src/hooks/usePanelWidth.ts";

test("panel widths clamp to the limits and round to whole pixels", () => {
  assert.equal(clampPanelWidth(0), PANEL_MIN_WIDTH);
  assert.equal(clampPanelWidth(-50), PANEL_MIN_WIDTH);
  assert.equal(clampPanelWidth(10_000), PANEL_MAX_WIDTH);
  assert.equal(clampPanelWidth(312.4), 312);
  assert.equal(clampPanelWidth(312.6), 313);
  assert.equal(clampPanelWidth(PANEL_MIN_WIDTH), PANEL_MIN_WIDTH);
  assert.equal(clampPanelWidth(PANEL_MAX_WIDTH), PANEL_MAX_WIDTH);
});

test("a stored panel width is null or a number within the limits", () => {
  for (const value of [null, PANEL_MIN_WIDTH, 320, PANEL_MAX_WIDTH])
    assert.equal(isPanelWidth(value), true, String(value));
  for (const value of [
    undefined, "320", PANEL_MIN_WIDTH - 1, PANEL_MAX_WIDTH + 1, NaN, Infinity, {}, [320], true,
  ])
    assert.equal(isPanelWidth(value), false, String(value));
  // What useLocalStorage reads back from JSON.
  assert.equal(isPanelWidth(JSON.parse("280")), true);
  assert.equal(isPanelWidth(JSON.parse("null")), true);
  assert.equal(isPanelWidth(JSON.parse('"wide"')), false);
});

const close = (actual: number, expected: number) =>
  assert.ok(Math.abs(actual - expected) < 1e-9, `${actual} ≠ ${expected}`);
/** Host point of scene point `p` under transform `t`. */
const screen = (t: Transform, p: { x: number; y: number }) => ({
  x: t.x + p.x * t.scale,
  y: t.y + p.y * t.scale,
});

test("pointerPair: midpoint and spread of two pointers; one pointer has no spread", () => {
  assert.deepEqual(pointerPair([{ x: 0, y: 0 }, { x: 30, y: 40 }]), { mid: { x: 15, y: 20 }, distance: 50 });
  assert.deepEqual(pointerPair([{ x: 7, y: 9 }]), { mid: { x: 7, y: 9 }, distance: 0 });
  // Only the first two pointers count.
  assert.equal(pointerPair(new Map([[1, { x: 0, y: 0 }], [2, { x: 10, y: 0 }], [3, { x: 99, y: 0 }]]).values()).distance, 10);
});

test("pinchStep: spreading fingers zooms about the old midpoint and follows the new one", () => {
  const before = pointerPair([{ x: 100, y: 100 }, { x: 200, y: 100 }]);
  const apart = pinchStep(before, pointerPair([{ x: 50, y: 100 }, { x: 250, y: 100 }]))!;
  assert.deepEqual(apart, { factor: 2, from: { x: 150, y: 100 }, to: { x: 150, y: 100 } });
  const together = pinchStep(before, pointerPair([{ x: 125, y: 100 }, { x: 175, y: 100 }]))!;
  assert.equal(together.factor, 0.5);
  // A two-finger drag pans without zooming.
  const pan = pinchStep(before, pointerPair([{ x: 110, y: 130 }, { x: 210, y: 130 }]))!;
  assert.deepEqual(pan, { factor: 1, from: { x: 150, y: 100 }, to: { x: 160, y: 130 } });
  assert.equal(pinchStep(before, pointerPair([{ x: 100, y: 100 }, { x: 200, y: 100 }])), undefined);
  // Coincident fingers have no spread to divide by.
  const pinned = pointerPair([{ x: 5, y: 5 }, { x: 5, y: 5 }]);
  assert.equal(pinchStep(pinned, pointerPair([{ x: 0, y: 5 }, { x: 10, y: 5 }]))?.factor ?? 1, 1);
});

test("zoomTransform keeps the scene point under the pinch midpoint", () => {
  const old: Transform = { x: 40, y: -20, scale: 0.5 };
  const point = { x: 300, y: 200 };
  const scenePoint = { x: (point.x - old.x) / old.scale, y: (point.y - old.y) / old.scale };
  const zoomed = zoomTransform(old, 2, point);
  close(zoomed.scale, 1);
  close(screen(zoomed, scenePoint).x, point.x);
  close(screen(zoomed, scenePoint).y, point.y);
  // With a moved midpoint, the same scene point lands under `to`.
  const to = { x: 320, y: 260 };
  const moved = zoomTransform(old, 1.5, point, to);
  close(screen(moved, scenePoint).x, to.x);
  close(screen(moved, scenePoint).y, to.y);
});

test("zoomTransform clamps the scale and anchors at the clamped scale", () => {
  const old: Transform = { x: 0, y: 0, scale: 3 };
  const point = { x: 100, y: 100 };
  const max = zoomTransform(old, 10, point);
  assert.equal(max.scale, clampScale(Infinity));
  close(screen(max, { x: point.x / 3, y: point.y / 3 }).x, point.x);
  assert.equal(zoomTransform(old, 1e-6, point).scale, clampScale(0));
  assert.equal(zoomTransform(old, 1e-6, point, point, 0.25).scale, 0.25);
});

/** A PointerGesture whose handlers record what they were asked to do. */
function recordingGesture() {
  const calls = { zooms: [] as { factor: number; from: Point; to: Point }[], starts: 0, ends: 0 };
  const handlers: CanvasGestureHandlers = {
    zoom: (factor, from, to) => calls.zooms.push({ factor, from, to }),
    start: () => calls.starts++,
    end: () => calls.ends++,
  };
  return { gesture: new PointerGesture(() => handlers), calls };
}

test("a pinch starts from where the first finger is when the second lands, not where it went down", () => {
  const { gesture, calls } = recordingGesture();
  gesture.down(1, { x: 100, y: 100 }, true);
  // One-finger pan: the canvas pans; the gesture only follows the finger.
  for (let x = 110; x <= 300; x += 10) gesture.move(1, { x, y: 100 });
  assert.equal(calls.zooms.length, 0);
  gesture.down(2, { x: 400, y: 100 }, false);
  assert.equal(calls.starts, 1);
  assert.equal(gesture.engaged, true);
  // Spreading the second finger by 1 px is a tiny zoom about the current midpoint (350, 100).
  gesture.move(2, { x: 401, y: 100 });
  assert.equal(calls.zooms.length, 1);
  const [step] = calls.zooms;
  close(step.factor, 101 / 100);
  assert.deepEqual(step.from, { x: 350, y: 100 });
  assert.deepEqual(step.to, { x: 350.5, y: 100 });
  // Moving the first finger 1 px is as small a step. With its touch-down point kept, this was
  // a jump from (100, 100) to (299, 100): a 0.34x zoom and a 100 px pan.
  gesture.move(1, { x: 299, y: 100 });
  const last = calls.zooms[1];
  close(last.factor, 102 / 101);
  assert.deepEqual(last.from, { x: 350.5, y: 100 });
  assert.deepEqual(last.to, { x: 350, y: 100 });
});

test("start and end pair up even when the last pointer-up was never seen", () => {
  const { gesture, calls } = recordingGesture();
  gesture.down(1, { x: 0, y: 0 }, true);
  gesture.down(2, { x: 50, y: 0 }, false);
  assert.deepEqual([calls.starts, calls.ends], [1, 0]);
  // Both ups are lost; the next touch is a new primary pointer.
  gesture.down(3, { x: 10, y: 10 }, true);
  assert.equal(calls.ends, 1, "the previous gesture is ended (Konva drag / canvas pan restored)");
  assert.equal(gesture.engaged, false);
  assert.deepEqual([...gesture.pointers.keys()], [3]);
  // A cancel (or lost capture) of every pointer ends a gesture once.
  gesture.down(4, { x: 60, y: 10 }, false);
  assert.equal(calls.starts, 2);
  gesture.up(3);
  gesture.up(4);
  gesture.up(4);
  assert.equal(calls.ends, 2);
  // The pointer-up that ended it is still not a tap; the next first pointer resets that.
  assert.equal(gesture.engaged, true);
  gesture.down(5, { x: 0, y: 0 }, true);
  assert.deepEqual([calls.ends, gesture.engaged], [2, false]);
  // One-pointer gestures never start or end.
  gesture.up(5);
  assert.deepEqual([calls.starts, calls.ends], [2, 2]);
});

test("panDragStep: nothing within the click slop, then the whole movement, then per-move deltas", () => {
  const drag = startPanDrag({ x: 10, y: 10 });
  assert.equal(panDragStep(drag, { x: 13, y: 13 }, 5), undefined);
  assert.equal(drag.moved, false);
  assert.deepEqual(panDragStep(drag, { x: 20, y: 10 }, 5), { x: 10, y: 0 });
  assert.equal(drag.moved, true);
  assert.deepEqual(panDragStep(drag, { x: 22, y: 7 }, 5), { x: 2, y: -3 });
  assert.equal(panDragStep(drag, { x: 22, y: 7 }, 5), undefined);
  // Back inside the slop it is still a pan.
  assert.deepEqual(panDragStep(drag, { x: 10, y: 10 }, 5), { x: -12, y: 3 });
  const old: Transform = { x: 1, y: 2, scale: 3 };
  assert.equal(panTransform(old, { x: 0, y: 0 }), old);
  assert.deepEqual(panTransform(old, { x: 4, y: -2 }), { x: 5, y: 0, scale: 3 });
});

/** Scene point under host point `p`. */
const world = (t: Transform, p: Point) => ({ x: (p.x - t.x) / t.scale, y: (p.y - t.y) / t.scale });
const wheelEvent = (deltaY: number, ctrlKey = false) =>
  ({ deltaY, deltaMode: 0, ctrlKey }) as WheelEvent;

/**
 * A drag interleaved with a dense smooth-scroll stream (Mac Mouse Fix: many small pixel
 * deltas, momentum tailing off after the wheel stops) and trackpad pinch (ctrl+wheel). Each
 * event is a camera update; `flushEvery` queues them as React state updaters and applies them
 * in batches, as React does between renders.
 */
function dragWhileScrolling(flushEvery: number) {
  let camera: Transform = { x: 60, y: 60, scale: 1 };
  const queue: ((old: Transform) => Transform)[] = [];
  const update = (step: (old: Transform) => Transform) => {
    queue.push(step);
    if (queue.length >= flushEvery) flush();
  };
  const flush = () => {
    for (const step of queue.splice(0)) camera = step(camera);
  };
  let cursor = { x: 200, y: 150 };
  const grabbed = world(camera, cursor);
  const drag = startPanDrag(cursor);
  let worst = 0;
  for (let i = 0; i < 400; i++) {
    cursor = { x: cursor.x + 3 + (i % 7), y: cursor.y + ((i % 11) - 5) };
    const at = cursor;
    const delta = panDragStep(drag, at, 5);
    if (delta) update((old) => panTransform(old, delta));
    // Several wheel events per pointer move: a burst, then momentum decaying to nothing.
    const burst = i < 150 ? 6 : i < 300 ? 3 : 1;
    for (let j = 0; j < burst; j++) {
      const deltaY = i < 150 ? -(2 + ((i + j) % 6)) : i < 300 ? 4 * Math.exp(-(i - 150) / 40) : 0;
      const factor = wheelZoomFactor(wheelEvent(deltaY, i % 50 === 0));
      if (factor !== 1) update((old) => zoomTransform(old, factor, at, at, 0.0001));
    }
    flush();
    const shown = screen(camera, grabbed);
    worst = Math.max(worst, Math.hypot(shown.x - cursor.x, shown.y - cursor.y));
  }
  return { camera, worst };
}

test("wheel zoom during a drag composes with it: the grabbed point stays under the cursor", () => {
  for (const flushEvery of [1, 4, 1000]) {
    const { camera, worst } = dragWhileScrolling(flushEvery);
    assert.ok(camera.scale > 2, `zoomed in during the drag (${camera.scale})`);
    assert.ok(worst < 1e-6, `grabbed point drifted ${worst}px from the cursor (batch ${flushEvery})`);
  }
});

test("the old drag (start camera + total movement) undid wheel zoom made while dragging", () => {
  // Regression guard for the bug itself: absolute drag positions from a camera snapshot taken
  // at drag start jump as soon as a wheel event zooms mid-drag.
  let camera: Transform = { x: 60, y: 60, scale: 1 };
  const start = { x: 200, y: 150 };
  const snapshot = { ...camera };
  const grabbed = world(camera, start);
  camera = zoomTransform(camera, 1.5, { x: 220, y: 150 });
  const cursor = { x: 230, y: 150 };
  const absolute = { ...camera, x: snapshot.x + cursor.x - start.x, y: snapshot.y + cursor.y - start.y };
  const shown = screen(absolute, grabbed);
  assert.ok(Math.hypot(shown.x - cursor.x, shown.y - cursor.y) > 50);
  // The delta drag keeps it under the cursor.
  const drag = startPanDrag(start);
  panDragStep(drag, { x: 220, y: 150 }, 5);
  let fixed: Transform = panTransform({ x: 60, y: 60, scale: 1 }, { x: 20, y: 0 });
  fixed = zoomTransform(fixed, 1.5, { x: 220, y: 150 });
  fixed = panTransform(fixed, panDragStep(drag, cursor, 5)!);
  close(screen(fixed, grabbed).x, cursor.x);
  close(screen(fixed, grabbed).y, cursor.y);
});
