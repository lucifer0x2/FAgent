/** A box to pack: its height includes the label or title room reserved above its content. */
export interface SkylineBox {
  width: number;
  height: number;
}
/**
 * A packed box: `x`, `y` is its top-left corner; `above` is the free distance from that top
 * to the bottom of the nearest box over it (`Infinity` when nothing is over it).
 */
export interface SkylinePlace {
  x: number;
  y: number;
  above: number;
}
export interface SkylineLayout {
  places: SkylinePlace[];
  width: number;
  height: number;
}

const EPS = 1e-6;

/** One step of the skyline: the next free top over `[x, end)`, gap below the box above included. */
interface Step {
  x: number;
  end: number;
  y: number;
}

/**
 * Pack `boxes` in authored order into `limit` width, `gap` apart, each at the highest free
 * spot on the skyline (then leftmost), so short boxes stack beside a tall one instead of
 * starting a row under it. Boxes never fill holes under the skyline, and a box never sits
 * above and left of an earlier box's top-left corner, so the authored order reads left to
 * right, top to bottom. Deterministic; O(boxes × steps) per box.
 */
export function skyline(boxes: SkylineBox[], limit: number, gap: number): SkylineLayout {
  // Each box occupies its gap to the right and below: a `limit + gap` wide strip.
  const right = Math.max(limit, ...boxes.map((box) => box.width)) + gap;
  let steps: Step[] = [{ x: 0, end: right, y: 0 }];
  // Top-left corners not dominated by a later one (x ascending, y descending).
  let corners: { x: number; y: number }[] = [];
  const places: SkylinePlace[] = [];
  let width = 0,
    height = 0;
  for (const box of boxes) {
    const span = box.width + gap;
    let best: { x: number; y: number; floor: number } | undefined;
    for (let k = 0; k < steps.length; k++) {
      const x = steps[k].x;
      if (x + span > right + EPS) break;
      if (best && steps[k].y >= best.y - EPS) continue;
      // Reading order: nothing earlier may start right of and below this corner.
      let top = corners.find((entry) => entry.x > x + EPS)?.y ?? 0,
        floor = 0;
      for (let j = k; j < steps.length && steps[j].x < x + span - EPS; j++) {
        top = Math.max(top, steps[j].y);
        // Only steps under the box itself (not its right gap) bound the label room.
        if (steps[j].x < x + box.width - EPS) floor = Math.max(floor, steps[j].y);
        if (best && top >= best.y - EPS) break;
      }
      if (!best || top < best.y - EPS) best = { x, y: top, floor };
    }
    const { x, y, floor } = best!;
    places.push({ x, y, above: floor > 0 ? y - (floor - gap) : Infinity });
    steps = raise(steps, x, x + span, y + box.height + gap);
    corners = corners.filter((entry) => entry.x > x + EPS || entry.y > y + EPS);
    corners.push({ x, y });
    corners.sort((a, b) => a.x - b.x);
    width = Math.max(width, x + box.width);
    height = Math.max(height, y + box.height);
  }
  return { places, width, height };
}

/** The skyline with `[from, to)` raised to `y`, equal neighbours merged. */
function raise(steps: Step[], from: number, to: number, y: number): Step[] {
  const next: Step[] = [];
  const push = (x: number, end: number, top: number) => {
    if (end - x <= EPS) return;
    const last = next.at(-1);
    if (last && Math.abs(last.y - top) <= EPS) last.end = end;
    else next.push({ x, end, y: top });
  };
  for (const step of steps)
    if (step.x < from - EPS) push(step.x, Math.min(step.end, from), step.y);
  push(from, to, y);
  for (const step of steps)
    if (step.end > to + EPS) push(Math.max(step.x, to), step.end, step.y);
  return next;
}