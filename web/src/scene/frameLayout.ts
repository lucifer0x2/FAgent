import { skyline } from "./skyline.ts";

export interface FrameSize {
  id: string;
  width: number;
  height: number;
}
export interface FramePlacement extends FrameSize {
  x: number;
  y: number;
  /** Free board units above the frame for its label, up to whatever sits over it (`Infinity`
   * when nothing does: such a label can always show). */
  labelSpace?: number;
}
export interface FrameBoard {
  frames: FramePlacement[];
  width: number;
  height: number;
}
export const FRAME_GAP = 64;
export const FRAME_LABEL = 32;

/**
 * The screen area (CSS px, after the Fit margins) boards are laid out for: 16:10, the shape
 * of common laptop screens, sized like the canvas pane of a 1440×900 window with both
 * drawers open. Its aspect (1.6) sits between that pane (~1.1) and the pane with the drawers
 * closed (~1.9), and its size sets label/title room so they read at Fit on such panes. Board
 * positions depend on it and the content only, never on the live pane size.
 */
export const REFERENCE_VIEW = { width: 800, height: 500 };
export const REFERENCE_ASPECT = REFERENCE_VIEW.width / REFERENCE_VIEW.height;
// Screen px of label room at the reference Fit (12 px text, 24 px drawn band, spacing).
const LABEL_SCREEN = 28;
// Bounded target-width search, log-spaced between the widest frame and one long row.
const CANDIDATES = 40;

function place(frames: FrameSize[], limit: number, labelHeight: number, gap: number): FrameBoard {
  const packed = skyline(
    frames.map((frame) => ({ width: frame.width, height: frame.height + labelHeight })),
    limit,
    gap,
  );
  return {
    frames: frames.map((frame, index) => {
      const at = packed.places[index];
      return { ...frame, x: at.x, y: at.y + labelHeight, labelSpace: labelHeight + at.above };
    }),
    width: packed.width,
    height: packed.height,
  };
}

/**
 * Stable authored order, real dimensions, a label band above each frame. Skyline packing
 * (short frames stack beside tall ones) into the target width, chosen by a bounded search
 * that scores the Fit envelope at `aspect` and the wasted area.
 */
export function packFrames(
  frames: FrameSize[],
  aspect = REFERENCE_ASPECT,
  labelHeight = FRAME_LABEL,
  gap = FRAME_GAP,
): FrameBoard {
  if (!frames.length) return { frames: [], width: 0, height: 0 };
  const ratio = Math.max(0.1, Math.min(10, aspect || REFERENCE_ASPECT));
  const min = frames.reduce((max, frame) => Math.max(max, frame.width), 0);
  const max = frames.reduce((sum, frame) => sum + frame.width + gap, -gap);
  const candidates = new Set([min, max]);
  for (let i = 1; i < CANDIDATES; i++)
    candidates.add(min * Math.pow(max / min, i / CANDIDATES));
  const area = frames.reduce(
    (sum, frame) => sum + frame.width * (frame.height + labelHeight),
    0,
  );
  let best: FrameBoard | undefined,
    score = Infinity;
  for (const limit of [...candidates].sort((a, b) => a - b)) {
    const layout = place(frames, limit, labelHeight, gap);
    // The score is at least width / ratio, and wider limits pack wider: none can win now.
    if (layout.width / ratio > score) break;
    const envelope = Math.max(layout.width / ratio, layout.height);
    const wasted = 1 - area / (layout.width * layout.height);
    const smallRowPenalty =
      frames.length <= 3 &&
      ratio >= 1 &&
      layout.frames.some((frame) => frame.y !== layout.frames[0].y)
        ? 0.3
        : 0;
    const next = envelope * (1 + wasted * 0.12 + smallRowPenalty);
    if (next < score - 1e-9) {
      best = layout;
      score = next;
    }
  }
  return best!;
}

/**
 * Board units per screen px when a `width` × `height` board is fitted into the reference
 * view (at most 100%, as Fit).
 */
export function referenceUnit(width: number, height: number) {
  return Math.max(1, width / REFERENCE_VIEW.width, height / REFERENCE_VIEW.height);
}

/**
 * A page's frames on one board. Label room is sized to read at the board's Fit in the
 * reference view, so positions depend only on the frames, never on the pane size or zoom.
 */
export function fitFrameBoard(frames: FrameSize[]): FrameBoard {
  let labelHeight = FRAME_LABEL;
  let layout = packFrames(frames, REFERENCE_ASPECT, labelHeight);
  for (let i = 0; i < 16 && frames.length; i++) {
    const desired = Math.max(FRAME_LABEL, LABEL_SCREEN * referenceUnit(layout.width, layout.height));
    if (desired <= labelHeight + 0.01) break;
    // Monotonic clearance avoids oscillation when the best arrangement changes; the 10%
    // headroom absorbs the board growing with its labels, so it settles in a few passes.
    labelHeight = desired * 1.1;
    layout = packFrames(frames, REFERENCE_ASPECT, labelHeight);
  }
  return layout;
}
