import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { KeyboardEvent } from "react";

export interface Transform {
  x: number;
  y: number;
  scale: number;
}
export interface Point {
  x: number;
  y: number;
}
interface Viewport {
  width: number;
  height: number;
}

const MIN_SCALE = 0.05;
const MAX_SCALE = 4;
const STEP = 1.2;
// Scale exponent per wheel pixel: a mouse notch still steps noticeably, a trackpad glides.
const WHEEL_SPEED = 0.0012;
const PINCH_SPEED = 0.0035;
// Room left around a fitted frame for the floating chrome.
const FIT_MARGIN = { x: 96, y: 120 };
const PAN_STEP: Record<string, [number, number]> = {
  ArrowUp: [0, 40],
  ArrowDown: [0, -40],
  ArrowLeft: [40, 0],
  ArrowRight: [-40, 0],
};

export const clampScale = (value: number, minimum = MIN_SCALE) =>
  Math.max(minimum, Math.min(MAX_SCALE, value));

/** Zoom factor for one wheel or trackpad step, proportional to the delta that step carries. */
export function wheelZoomFactor(wheel: WheelEvent): number {
  // Lines and pages carry far fewer units than pixels; even them out first.
  const unit = wheel.deltaMode === 1 ? 16 : wheel.deltaMode === 2 ? 120 : 1;
  const delta = Math.max(-80, Math.min(80, wheel.deltaY * unit));
  if (!delta) return 1;
  return Math.exp(-delta * (wheel.ctrlKey ? PINCH_SPEED : WHEEL_SPEED));
}

/**
 * `old` zoomed by `factor` about host point `point`: the scene position under `point` ends up
 * under `to`, which is `point` itself unless a pinch's midpoint also moved.
 */
export function zoomTransform(
  old: Transform,
  factor: number,
  point: Point,
  to: Point = point,
  minimumScale = MIN_SCALE,
): Transform {
  const scale = clampScale(old.scale * factor, minimumScale);
  return {
    scale,
    x: to.x - ((point.x - old.x) / old.scale) * scale,
    y: to.y - ((point.y - old.y) / old.scale) * scale,
  };
}

/** `old` moved by a pan delta in host pixels. */
export const panTransform = (old: Transform, delta: Point): Transform =>
  delta.x || delta.y ? { ...old, x: old.x + delta.x, y: old.y + delta.y } : old;

/** Pan and zoom of the canvas viewport, with the host element it is measured against. */
export function useCanvasTransform(
  content: Viewport | undefined | ((size: Viewport) => Viewport | undefined),
  cameraKey: string | undefined,
  minimumScale = MIN_SCALE,
) {
  const host = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  const viewport = typeof content === "function" ? content(size) : content;
  const fitted = useRef({ cameraKey, done: false });
  const [transform, setTransform] = useState<Transform>({
    x: 60,
    y: 60,
    scale: 1,
  });

  useEffect(() => {
    const observer = new ResizeObserver(([entry]) =>
      setSize((old) =>
        old.width === entry.contentRect.width && old.height === entry.contentRect.height
          ? old
          : { width: entry.contentRect.width, height: entry.contentRect.height },
      ),
    );
    if (host.current) observer.observe(host.current);
    return () => observer.disconnect();
  }, []);

  const viewportWidth = viewport?.width;
  const viewportHeight = viewport?.height;
  const fit = useCallback(() => {
    if (
      viewportWidth === undefined || viewportHeight === undefined ||
      size.width <= 0 || size.height <= 0
    ) return;
    const scale = clampScale(
      Math.min(
        (size.width - FIT_MARGIN.x) / viewportWidth,
        (size.height - FIT_MARGIN.y) / viewportHeight,
        1,
      ),
      minimumScale,
    );
    setTransform({
      x: (size.width - viewportWidth * scale) / 2,
      y: (size.height - viewportHeight * scale) / 2,
      scale,
    });
  }, [viewportWidth, viewportHeight, size.width, size.height, minimumScale]);
  useLayoutEffect(() => {
    if (fitted.current.cameraKey !== cameraKey)
      fitted.current = { cameraKey, done: false };
    if (
      !fitted.current.done && viewportWidth !== undefined && viewportHeight !== undefined &&
      size.width > 0 && size.height > 0
    ) {
      fit();
      fitted.current.done = true;
    }
  }, [cameraKey, viewportWidth, viewportHeight, size.width, size.height, fit]);

  /** Fit a board rectangle (e.g. one page of the All pages board) into the viewport. */
  const fitTo = useCallback(
    (rect: { x: number; y: number; width: number; height: number }) => {
      if (size.width <= 0 || size.height <= 0 || rect.width <= 0 || rect.height <= 0) return;
      const scale = clampScale(
        Math.min(
          (size.width - FIT_MARGIN.x) / rect.width,
          (size.height - FIT_MARGIN.y) / rect.height,
          1,
        ),
        minimumScale,
      );
      setTransform({
        x: (size.width - rect.width * scale) / 2 - rect.x * scale,
        y: (size.height - rect.height * scale) / 2 - rect.y * scale,
        scale,
      });
    },
    [size.width, size.height, minimumScale],
  );

  /**
   * Zoom by `factor` about `point` (host coordinates): the scene position under `point`
   * ends up under `to`, which is `point` itself unless a pinch's midpoint also moved.
   */
  const zoom = useCallback(
    (factor: number, point: Point = { x: size.width / 2, y: size.height / 2 }, to: Point = point) =>
      setTransform((old) => zoomTransform(old, factor, point, to, minimumScale)),
    [size.width, size.height, minimumScale],
  );

  /**
   * Pan by a delta on the latest camera. Pans, wheel zoom and pinch steps are all relative
   * updates of the current camera, so interleaved ones compose rather than overwrite.
   */
  const panBy = useCallback(
    (delta: Point) => setTransform((old) => panTransform(old, delta)),
    [],
  );

  const handleKeyDown = useCallback((event: KeyboardEvent) => {
    const pan = PAN_STEP[event.key];
    const key = event.key.toLowerCase();
    if (pan || ["+", "=", "-", "f"].includes(key)) event.preventDefault();
    if (event.key === "+" || event.key === "=") zoom(STEP);
    if (event.key === "-") zoom(1 / STEP);
    if (key === "f") fit();
    if (pan) panBy({ x: pan[0], y: pan[1] });
  }, [zoom, fit, panBy]);

  return {
    host,
    size,
    viewport,
    transform,
    zoom,
    fit,
    fitTo,
    panBy,
    handleKeyDown,
    STEP,
  };
}
