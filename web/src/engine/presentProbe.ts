import type { EngineApi } from "./engineApi.ts";

/**
 * WebGPU present probe, run once when a WebGPU surface is attached (and rebuilt).
 *
 * Vello's renderer probe draws into an offscreen texture, so it passes even when the canvas
 * path does not work: configure → getCurrentTexture → submit → transferToImageBitmap. That
 * path is what the page shows. This probe fills a small canvas with two known opaque colours
 * through the board's present code and reads each image back, so a canvas that stays blank,
 * keeps a stale frame or swaps channels is caught before anything is drawn. It never looks
 * at page content, so an empty or transparent board cannot fail it.
 */
export const PRESENT_PROBE_SIZE = 16;
/** Two frames with distinct, channel-asymmetric colours (0xRRGGBB). */
export const PRESENT_PROBE_COLORS = [0x2080e0, 0xe06020] as const;
const TOLERANCE = 24;
/** Inset corners and centre of the probe square. */
const POINTS = [
  [2, 2],
  [13, 2],
  [2, 13],
  [13, 13],
  [8, 8],
] as const;
const IDLE_WAIT_MS = 1_000;

export interface PresentProbeDeps {
  /** Straight RGBA8 of `image` (PRESENT_PROBE_SIZE square), e.g. via a 2D canvas. */
  samplePixels(image: ImageBitmap): ArrayLike<number>;
  yieldTask(): Promise<void>;
  now(): number;
}

const hex = (rgb: number) => `#${rgb.toString(16).padStart(6, "0")}`;

/** Why `pixels` (RGBA8, PRESENT_PROBE_SIZE wide) do not show opaque `rgb`, or undefined. */
export function presentMismatch(pixels: ArrayLike<number>, rgb: number) {
  const want = [(rgb >> 16) & 255, (rgb >> 8) & 255, rgb & 255];
  for (const [x, y] of POINTS) {
    const i = (y * PRESENT_PROBE_SIZE + x) * 4;
    const got = [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]];
    if (got.some((v) => typeof v !== "number")) return "canvas image has no pixels";
    if (got[3] < 250) return `canvas stayed blank (alpha ${got[3]} at ${x},${y})`;
    if (want.some((v, c) => Math.abs(v - got[c]) > TOLERANCE)) {
      const seen = (got[0] << 16) | (got[1] << 8) | got[2];
      return `canvas shows ${hex(seen)} instead of ${hex(rgb)} at ${x},${y}`;
    }
  }
  return undefined;
}

/** Throws when the WebGPU canvas does not show what was presented, or the GPU errored. */
export async function presentProbe(
  engine: EngineApi,
  canvas: OffscreenCanvas,
  deps: PresentProbeDeps,
) {
  try {
    for (const rgb of PRESENT_PROBE_COLORS) {
      engine.present_probe(PRESENT_PROBE_SIZE, rgb);
      const image = canvas.transferToImageBitmap();
      let problem: string | undefined;
      try {
        problem = presentMismatch(deps.samplePixels(image), rgb);
      } finally {
        image.close?.();
      }
      if (problem) throw new Error(`present probe: ${problem}`);
    }
    // Uncaptured errors arrive as events: let the probe frames finish first.
    const deadline = deps.now() + IDLE_WAIT_MS;
    while (!engine.gpu_idle() && deps.now() < deadline) await deps.yieldTask();
    await deps.yieldTask();
    const error = engine.gpu_error();
    if (error) throw new Error(`GPU error: ${error}`);
  } finally {
    try {
      engine.end_present_probe();
    } catch {
      // An aborted module rethrows here; keep the probe's own error.
    }
  }
}
