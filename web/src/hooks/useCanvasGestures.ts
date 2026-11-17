import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { RefObject } from "react";
import { wheelZoomFactor, type Point } from "./useCanvasTransform.ts";
import { PointerGesture, type CanvasGestureHandlers } from "./canvasPointers.ts";

export {
  pinchStep,
  pointerPair,
  type CanvasGestureHandlers,
  type PointerPair,
  type ZoomStep,
} from "./canvasPointers.ts";

/**
 * Wheel/trackpad zoom and touch gestures on the canvas host: two pointers pinch-zoom about
 * their midpoint and pan as it moves; after a pinch, a remaining finger keeps panning.
 * One-pointer pan and taps stay with the canvas; `gesturing()` is true from the second
 * pointer down until the next first pointer down, so the last pointer-up is not a tap.
 * Host listeners run before React's delegated handlers, so those see the current state.
 */
export function useCanvasGestures(
  host: RefObject<HTMLElement | null>,
  handlers: CanvasGestureHandlers,
) {
  const latest = useRef(handlers);
  useLayoutEffect(() => {
    latest.current = handlers;
  });
  const [gesture] = useState(() => new PointerGesture(() => latest.current));

  useEffect(() => {
    const element = host.current;
    if (!element) return;
    const at = (event: { clientX: number; clientY: number }): Point => {
      const rect = element.getBoundingClientRect();
      return { x: event.clientX - rect.left, y: event.clientY - rect.top };
    };
    // Wheel and ctrl+wheel (trackpad pinch) zoom about the cursor, as a step on the latest
    // camera, so they compose with a pan drag in progress.
    const wheel = (event: WheelEvent) => {
      event.preventDefault();
      const factor = wheelZoomFactor(event);
      const point = at(event);
      if (factor !== 1) latest.current.zoom(factor, point, point);
    };
    const down = (event: PointerEvent) => gesture.down(event.pointerId, at(event), event.isPrimary);
    const move = (event: PointerEvent) => gesture.move(event.pointerId, at(event));
    const up = (event: PointerEvent) => gesture.up(event.pointerId);
    // The host itself lost capture without an up (a child losing its implicit touch capture
    // to the host is not an end: that pointer is still down).
    const lost = (event: PointerEvent) => {
      if (event.target === element) gesture.up(event.pointerId);
    };
    // iOS Safari: keep its own page pinch-zoom off the canvas even where touch-action is ignored.
    const prevent = (event: Event) => event.preventDefault();
    const listeners: [string, (event: never) => void][] = [
      ["pointerdown", down],
      ["pointermove", move],
      ["pointerup", up],
      ["pointercancel", up],
      ["lostpointercapture", lost],
    ];
    element.addEventListener("wheel", wheel, { passive: false });
    for (const [type, listener] of listeners)
      element.addEventListener(type, listener as EventListener);
    for (const type of ["gesturestart", "gesturechange", "gestureend"])
      element.addEventListener(type, prevent, { passive: false });
    return () => {
      element.removeEventListener("wheel", wheel);
      for (const [type, listener] of listeners)
        element.removeEventListener(type, listener as EventListener);
      for (const type of ["gesturestart", "gesturechange", "gestureend"])
        element.removeEventListener(type, prevent);
    };
  }, [host, gesture]);

  /** Whether the current (or just ended) pointer gesture is a multi-pointer one. */
  const gesturing = useCallback(() => gesture.engaged, [gesture]);
  return { gesturing };
}
