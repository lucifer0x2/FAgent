import { useCallback, useEffect, useState } from "react";
import useLocalStorage from "./ultility/useLocalStorage";

/** Same breakpoint as `workspace.css`, where both side panels become overlays. */
const NARROW_QUERY = "(max-width: 800px)";

const isBoolean = (value: unknown): value is boolean =>
  typeof value === "boolean";

const narrowNow = () =>
  typeof window !== "undefined" && window.matchMedia(NARROW_QUERY).matches;

/**
 * Layers (left) and Inspector (right) visibility. Wide layouts keep the remembered
 * choice (`left-open` / `right-open`, default open). Narrow layouts start with both
 * overlays closed so the canvas gets touch input, keep the choice in memory only, and
 * show one overlay at a time; crossing the breakpoint re-applies these defaults.
 */
export function usePanelVisibility() {
  const [narrow, setNarrow] = useState(narrowNow);
  const [leftStored, setLeftStored] = useLocalStorage("left-open", true, isBoolean);
  const [rightStored, setRightStored] = useLocalStorage("right-open", true, isBoolean);
  const [overlay, setOverlay] = useState<"left" | "right" | null>(null);

  useEffect(() => {
    const media = window.matchMedia(NARROW_QUERY);
    const follow = () => {
      setNarrow(media.matches);
      setOverlay(null);
    };
    media.addEventListener("change", follow);
    return () => media.removeEventListener("change", follow);
  }, []);

  const leftOpen = narrow ? overlay === "left" : leftStored;
  const rightOpen = narrow ? overlay === "right" : rightStored;
  const toggleLeft = useCallback(() => {
    if (narrow) setOverlay((side) => (side === "left" ? null : "left"));
    else setLeftStored(!leftStored);
  }, [narrow, leftStored, setLeftStored]);
  const toggleRight = useCallback(() => {
    if (narrow) setOverlay((side) => (side === "right" ? null : "right"));
    else setRightStored(!rightStored);
  }, [narrow, rightStored, setRightStored]);

  return { leftOpen, rightOpen, toggleLeft, toggleRight };
}
