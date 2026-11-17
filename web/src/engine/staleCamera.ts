import { NO_MARGIN, overscanCamera, type Margin } from "./overscan.ts";
import type { Camera } from "./protocol.ts";

export interface CanvasPlacement {
  /** CSS transform (origin top-left); "" when the image already matches. */
  transform: string;
  /** CSS size of the drawn image; "" keeps the stylesheet's 100%. */
  width: string;
  height: string;
}

const NONE: CanvasPlacement = { transform: "", width: "", height: "" };

/**
 * Places an engine canvas image drawn with `drawn` where the page's `current` camera
 * shows the same board points (both in device px, `pixelRatio` device px per CSS px).
 * The image covers the viewport plus `margin` device px on each side (overscan), so even
 * when the cameras match it sits at −margin. While the worker computes a frame it cannot
 * redraw; this keeps the stale image aligned with labels, skeletons and outlines painted
 * on the main thread instead of letting it lag behind a pan or zoom, and the overscan
 * fills the strip a pan slides in.
 */
export function stalePlacement(
  drawn: Camera | undefined,
  current: Camera,
  pixelRatio: number,
  margin: Margin = NO_MARGIN,
): CanvasPlacement {
  if (!drawn || !(drawn.scale > 0)) return NONE;
  // The image's own camera: a board point p is at p·s + t in each camera, so device px q
  // of the image moves to (q − t₀)·k + t₁ with k = s₁ / s₀.
  const image = overscanCamera(drawn, margin);
  const k = current.scale / image.scale;
  const x = (current.tx - k * image.tx) / pixelRatio,
    y = (current.ty - k * image.ty) / pixelRatio;
  const resized = image.width !== current.width || image.height !== current.height;
  const moved = k !== 1 || x !== 0 || y !== 0;
  if (!resized && !moved) return NONE;
  return {
    transform: moved ? `translate(${x}px, ${y}px) scale(${k})` : "",
    // A canvas keeps its old pixel size until it redraws; never stretch it to the host.
    width: resized ? `${image.width / pixelRatio}px` : "",
    height: resized ? `${image.height / pixelRatio}px` : "",
  };
}
