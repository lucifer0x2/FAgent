import { useLayoutEffect, useRef, useState } from "react";
import type { KeyboardEvent, PointerEvent } from "react";
import {
  clampPanelWidth,
  PANEL_MIN_WIDTH,
  panelKeyWidth,
  visiblePanelMax,
} from "../hooks/usePanelWidth";
import "./panel-resizer.css";

const DRAGGING_CLASS = "is-resizing-panel";

interface Props {
  /** Which panel the handle resizes: the element right before it (left) or right after it. */
  side: "left" | "right";
  label: string;
  /** The committed width; null is the stylesheet default. */
  width: number | null;
  setWidth: (width: number | null) => void;
}

/**
 * The draggable inner edge of a side panel. While dragging it sizes the panel element directly
 * (no React render per move) and commits the final width once; arrow keys step by 16 px and a
 * double click restores the default width. Widths stay within what the stylesheet shows at this
 * viewport width, so the stored width, the edge and `aria-valuenow` agree.
 */
export function PanelResizer({ side, label, width, setWidth }: Props) {
  const handle = useRef<HTMLDivElement>(null);
  const drag = useRef<{ x: number; width: number; next: number; max: number } | null>(null);
  const [measured, setMeasured] = useState(width ?? PANEL_MIN_WIDTH);
  const [viewport, setViewport] = useState(() => window.innerWidth);
  const max = visiblePanelMax(viewport);
  // The width on screen: a stored width the stylesheet caps shows at the cap. The stored width
  // updates in the render after a key press, so quick presses do not wait for a resize event.
  const rendered = Math.min(max, width === null ? clampPanelWidth(measured) : width);

  const panel = () => {
    const element = handle.current;
    const sibling = side === "left" ? element?.previousElementSibling : element?.nextElementSibling;
    return sibling instanceof HTMLElement ? sibling : null;
  };
  const panelWidth = (element: HTMLElement) => parseFloat(getComputedStyle(element).width);

  // Follow the rendered width (defaults and the cap differ per viewport) for aria and steps.
  useLayoutEffect(() => {
    const element = panel();
    if (!element) return;
    const update = () => setMeasured(Math.round(panelWidth(element)));
    const resize = () => setViewport(window.innerWidth);
    update();
    resize();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    window.addEventListener("resize", resize);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", resize);
    };
  }, [side]);

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    const element = panel();
    if (event.button !== 0 || !element) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    const start = panelWidth(element);
    drag.current = { x: event.clientX, width: start, next: start, max: visiblePanelMax(window.innerWidth) };
    document.body.classList.add(DRAGGING_CLASS);
    event.currentTarget.classList.add("is-dragging");
  };
  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const state = drag.current, element = panel();
    if (!state || !element) return;
    const delta = (event.clientX - state.x) * (side === "left" ? 1 : -1);
    state.next = Math.min(state.max, clampPanelWidth(state.width + delta));
    element.style.width = `${state.next}px`;
  };
  const endDrag = () => {
    const state = drag.current;
    if (!state) return;
    drag.current = null;
    document.body.classList.remove(DRAGGING_CLASS);
    handle.current?.classList.remove("is-dragging");
    // Same task as the commit below, so the default width never paints in between.
    panel()?.style.removeProperty("width");
    if (Math.round(state.next) !== Math.round(state.width)) setWidth(state.next);
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const next = panelKeyWidth(event.key, rendered, side === "left" ? 1 : -1, max);
    if (next === undefined) return;
    event.preventDefault();
    setWidth(next);
  };

  return (
    <div
      ref={handle}
      className={`panel-resizer panel-resizer-${side}`}
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuemin={PANEL_MIN_WIDTH}
      aria-valuemax={max}
      aria-valuenow={rendered}
      tabIndex={0}
      title="Drag to resize, double-click to reset"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onLostPointerCapture={endDrag}
      onDoubleClick={() => setWidth(null)}
      onKeyDown={onKeyDown}
    />
  );
}
