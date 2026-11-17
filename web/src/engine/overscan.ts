import type { Camera } from "./protocol.ts";

/**
 * Overscan: the worker draws this fraction of the viewport beyond each edge, so an image
 * moved by `stalePlacement` during a pan still covers the strip that slides in until the
 * next image arrives (ratio 0.1: the image is ~1.2× the viewport per axis). Off by default;
 * the server sets it (`FAGENT_VIEWER_OVERSCAN`, percent per side) through `/api/v1/viewer`.
 */
export const DEFAULT_OVERSCAN = 0;
/** Largest ratio accepted (50% per side, as the server allows). */
export const MAX_OVERSCAN = 0.5;
/** Max canvas side when the backend reports none (vello_cpu, or before a surface exists). */
export const FALLBACK_MAX_TEXTURE = 4096;

/** A ratio from the server or a message: a finite number in [0, MAX_OVERSCAN], else 0. */
export function overscanRatio(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= MAX_OVERSCAN
    ? value
    : DEFAULT_OVERSCAN;
}

/** Device px drawn beyond each side of the viewport, per axis. */
export interface Margin {
  x: number;
  y: number;
}

export const NO_MARGIN: Margin = { x: 0, y: 0 };

/**
 * Margin for a `width` × `height` device px viewport: round(ratio × side) per axis,
 * shrunk so viewport + 2·margin fits `maxTexture` (0: unknown → FALLBACK_MAX_TEXTURE).
 * The viewport itself is never shrunk; one that alone exceeds the limit gets no margin.
 * Ratio 0 (the default) is no margin: the canvas is exactly the viewport.
 */
export function overscanMargin(
  width: number,
  height: number,
  maxTexture: number,
  ratio = DEFAULT_OVERSCAN,
): Margin {
  if (!(ratio > 0)) return NO_MARGIN;
  const limit = maxTexture > 0 ? maxTexture : FALLBACK_MAX_TEXTURE;
  const axis = (side: number) =>
    Math.max(0, Math.min(Math.round(ratio * side), Math.floor((limit - side) / 2)));
  return { x: axis(width), y: axis(height) };
}

/** The camera the worker draws with: the viewport grown by `margin` on each side. */
export const overscanCamera = (camera: Camera, margin: Margin): Camera => ({
  scale: camera.scale,
  tx: camera.tx + margin.x,
  ty: camera.ty + margin.y,
  width: camera.width + 2 * margin.x,
  height: camera.height + 2 * margin.y,
});
