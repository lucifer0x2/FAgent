import type { Point } from "./useCanvasTransform.ts";

/** Scale the camera by `factor` about host point `from`, then move that point to `to`. */
export type ZoomStep = (factor: number, from: Point, to: Point) => void;

export interface CanvasGestureHandlers {
  zoom: ZoomStep;
  /** A second pointer went down: the canvas's own one-pointer pan and tap must stop. */
  start?: () => void;
  /** The multi-pointer gesture is over: its last pointer went up, or a new gesture began. */
  end?: () => void;
}

export interface PointerPair {
  mid: Point;
  distance: number;
}

/** Midpoint and spread of the first two pointers (a lone pointer has no spread). */
export const pointerPair = (pointers: Iterable<Point>): PointerPair => {
  const [a, b] = pointers;
  return b
    ? { mid: { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }, distance: Math.hypot(a.x - b.x, a.y - b.y) }
    : { mid: a, distance: 0 };
};

/**
 * One pinch move as a zoom step: the spread ratio about the old midpoint, which then moves to
 * the new midpoint. Undefined when nothing changed.
 */
export function pinchStep(before: PointerPair, after: PointerPair) {
  const factor =
    before.distance > 0 && after.distance > 0 ? after.distance / before.distance : 1;
  if (factor === 1 && before.mid.x === after.mid.x && before.mid.y === after.mid.y) return;
  return { factor, from: before.mid, to: after.mid };
}

/**
 * The pointers down on the canvas host and the multi-pointer gesture they make, free of the
 * DOM so it can be driven by tests. Every pointer's latest position is kept from the moment
 * it goes down, so a pinch starts from where the fingers are when the second one lands.
 * `start` and `end` always come in pairs: a gesture whose last pointer-up was never seen is
 * ended when the next gesture begins.
 */
export class PointerGesture {
  readonly pointers = new Map<number, Point>();
  /** From the second pointer down until the next first pointer down. */
  engaged = false;
  /** Between `start` and `end`. */
  private active = false;
  private readonly handlers: () => CanvasGestureHandlers;

  constructor(handlers: () => CanvasGestureHandlers) {
    this.handlers = handlers;
  }

  down(id: number, at: Point, primary: boolean) {
    // A primary pointer starts a new gesture; drop pointers whose up was never seen.
    if (primary || !this.pointers.size) {
      this.pointers.clear();
      this.finish();
      this.engaged = false;
    }
    this.pointers.set(id, at);
    if (this.pointers.size === 2 && !this.engaged) {
      this.engaged = this.active = true;
      this.handlers().start?.();
    }
  }

  move(id: number, at: Point) {
    if (!this.pointers.has(id)) return;
    if (!this.engaged) {
      this.pointers.set(id, at);
      return;
    }
    const before = pointerPair(this.pointers.values());
    this.pointers.set(id, at);
    const step = pinchStep(before, pointerPair(this.pointers.values()));
    if (step) this.handlers().zoom(step.factor, step.from, step.to);
  }

  /** Pointer up, cancel or lost capture. */
  up(id: number) {
    if (this.pointers.delete(id) && !this.pointers.size) this.finish();
  }

  private finish() {
    if (!this.active) return;
    this.active = false;
    this.handlers().end?.();
  }
}

/** A one-pointer pan in progress: where it started and the last position applied. */
export interface PanDrag {
  start: Point;
  last: Point;
  /** Past the click slop: a pan, not a tap. */
  moved: boolean;
}

export const startPanDrag = (at: Point): PanDrag => ({ start: at, last: at, moved: false });

/**
 * The pan for a pointer move: the movement since the last applied position, nothing while
 * still within `slop` of the start. Applied as a delta to the latest camera, it composes with
 * wheel zoom and other camera changes made during the drag instead of overwriting them.
 */
export function panDragStep(drag: PanDrag, at: Point, slop: number): Point | undefined {
  if (!drag.moved) {
    if (Math.hypot(at.x - drag.start.x, at.y - drag.start.y) <= slop) return;
    drag.moved = true;
  }
  const delta = { x: at.x - drag.last.x, y: at.y - drag.last.y };
  drag.last = at;
  return delta.x || delta.y ? delta : undefined;
}
