import { useCallback } from "react";
import useLocalStorage from "./ultility/useLocalStorage.ts";

export const PANEL_MIN_WIDTH = 200;
export const PANEL_MAX_WIDTH = 500;
const KEY_STEP = 16;

export const clampPanelWidth = (width: number) =>
  Math.round(Math.min(PANEL_MAX_WIDTH, Math.max(PANEL_MIN_WIDTH, width)));

/**
 * The widest panel the wide layout shows at this viewport width: workspace.css caps both side
 * panels at `max(200px, calc(50vw - 160px))` to keep room for the canvas. Rounded down, so a
 * width at the cap never exceeds it.
 */
export const visiblePanelMax = (viewportWidth: number) =>
  Math.min(PANEL_MAX_WIDTH, Math.max(PANEL_MIN_WIDTH, Math.floor(viewportWidth / 2 - 160)));

/**
 * The width a resize key asks for, or undefined for other keys. Arrows step by 16 px from the
 * rendered width (not a stored width the stylesheet caps), and every result stays within what
 * the layout can show, so each press visibly moves the edge. `grow` is +1 when ArrowRight widens.
 */
export function panelKeyWidth(key: string, rendered: number, grow: 1 | -1, max: number) {
  const next =
    key === "ArrowRight" ? rendered + KEY_STEP * grow
    : key === "ArrowLeft" ? rendered - KEY_STEP * grow
    : key === "Home" ? PANEL_MIN_WIDTH
    : key === "End" ? max
    : undefined;
  return next === undefined ? undefined : Math.min(max, clampPanelWidth(next));
}

/** A stored width is valid when null (the default) or a number within the panel limits. */
export const isPanelWidth = (value: unknown): value is number | null =>
  value === null ||
  (typeof value === "number" &&
    value >= PANEL_MIN_WIDTH &&
    value <= PANEL_MAX_WIDTH);

/**
 * A side panel's user-chosen width, remembered per `storageKey` (best effort: unreadable or
 * invalid storage yields null). Null means the stylesheet's responsive default width.
 */
export function usePanelWidth(storageKey: string) {
  const [width, store] = useLocalStorage<number | null>(storageKey, null, isPanelWidth);
  const setWidth = useCallback(
    (next: number | null) => store(next === null ? null : clampPanelWidth(next)),
    [store],
  );
  return [width, setWidth] as const;
}
