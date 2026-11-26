import type { WorkerMessage } from "./protocol.ts";

/**
 * Pacing of the board redraws a frame load triggers. Every draw encodes the whole board
 * (~16 ms for ~230 frames at Fit), so drawing after each committed frame cost more than
 * computing the frames. While a sync computes, committed frames are shown together at most
 * once per gap; the gap grows with the last draw time so draws stay a bounded share of the
 * load, and is capped so frames still appear progressively.
 */

/** Shortest gap between two progress draws, ms. */
export const PROGRESS_DRAW_MIN_MS = 100;
/** Longest gap between two progress draws, ms (commit → pixels stays below this + one frame). */
export const PROGRESS_DRAW_MAX_MS = 250;
/** Gap as a multiple of the last draw time: progress draws take at most ~1/10 of a load. */
export const PROGRESS_DRAW_FACTOR = 10;

/** Gap before the next progress draw, given how long the last draw took. */
export function progressGap(lastDrawMs: number) {
  return Math.min(
    PROGRESS_DRAW_MAX_MS,
    Math.max(PROGRESS_DRAW_MIN_MS, lastDrawMs * PROGRESS_DRAW_FACTOR),
  );
}

/**
 * Holds the `frame` messages of committed frames until they are shown. The page drops a
 * frame's skeleton when its message arrives, so a message never goes out before an image
 * with the frame's pixels. Only a release changes what the page publishes (and, for frames
 * that hug their content, the board layout it sends back), so camera and layout draws in
 * between show the committed frames without releasing them: otherwise each layout draw
 * would release the next frames, whose heights change the layout again, one draw per frame.
 */
export class FramePacer {
  private held: WorkerMessage[] = [];
  private first = false;
  private releasedAt = -Infinity;
  private drawMs = 0;
  private now: () => number;
  private post: (message: WorkerMessage) => void;

  constructor(now: () => number, post: (message: WorkerMessage) => void) {
    this.now = now;
    this.post = post;
  }

  /** A new sync: its first committed frame is shown without waiting for the gap. */
  startSync() {
    this.first = true;
  }

  hold(message: WorkerMessage) {
    this.held.push(message);
  }

  pending() {
    return this.held.length > 0;
  }

  /** Held frames should be shown now: the first of a sync, or the gap has passed. */
  due() {
    return (
      this.held.length > 0 &&
      (this.first || this.now() - this.releasedAt >= progressGap(this.drawMs))
    );
  }

  /** Duration of the last draw, which sets the gap. */
  drew(ms: number) {
    this.drawMs = ms;
  }

  /** Posts the held messages: their frames are in the image posted next, or already shown. */
  release() {
    if (!this.held.length) return;
    this.first = false;
    this.releasedAt = this.now();
    for (const message of this.held.splice(0)) this.post(message);
  }
}
