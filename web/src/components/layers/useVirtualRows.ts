import { useCallback, useLayoutEffect, useRef, useState, type RefObject } from "react";

/** Rows rendered beyond each edge of the viewport, so scrolling and Tab stay ahead of paint. */
const OVERSCAN = 12;
/** Before the first measurement: enough rows for a tall panel. */
const INITIAL_ROWS = 60;

interface Range {
  start: number;
  end: number;
}

/**
 * A fixed-height window over `count` rows laid out from the top of `list`, inside the nearest
 * scrolling ancestor (the panel). Returns the rows to render and `reveal`, which centres a row
 * in the panel unless it is already fully visible.
 */
export function useVirtualRows(
  list: RefObject<HTMLElement | null>,
  count: number,
  rowHeight: number,
) {
  const [range, setRange] = useState<Range>({ start: 0, end: INITIAL_ROWS });
  const panel = useRef<HTMLElement | null>(null);

  /** The list's top within the panel's scrolled content. */
  const listTop = useCallback(() => {
    const scroller = panel.current, element = list.current;
    if (!scroller || !element) return 0;
    return (
      element.getBoundingClientRect().top -
      scroller.getBoundingClientRect().top +
      scroller.scrollTop
    );
  }, [list]);

  const measure = useCallback(() => {
    const scroller = panel.current;
    if (!scroller) return;
    const top = scroller.scrollTop - listTop();
    const start = Math.max(0, Math.floor(top / rowHeight) - OVERSCAN);
    const end = Math.max(
      start,
      Math.ceil((top + scroller.clientHeight) / rowHeight) + OVERSCAN,
    );
    setRange((old) => (old.start === start && old.end === end ? old : { start, end }));
  }, [listTop, rowHeight]);

  useLayoutEffect(() => {
    const scroller = scrollParent(list.current);
    panel.current = scroller;
    if (!scroller) return;
    measure();
    scroller.addEventListener("scroll", measure, { passive: true });
    const resize = new ResizeObserver(measure);
    resize.observe(scroller);
    return () => {
      scroller.removeEventListener("scroll", measure);
      resize.disconnect();
      panel.current = null;
    };
  }, [list, measure]);

  // Re-check after every commit, not only on scroll/resize events: those are delivered in the
  // browser's rendering steps, which a hidden or background page skips, so a scroll made there
  // (or a clamp when the list shrinks) would otherwise leave a stale, blank window. A no-op
  // when the range is unchanged.
  useLayoutEffect(measure);

  const reveal = useCallback(
    (row: number) => {
      const scroller = panel.current;
      if (!scroller) return;
      const top = listTop() + row * rowHeight;
      const viewTop = scroller.scrollTop;
      if (top >= viewTop && top + rowHeight <= viewTop + scroller.clientHeight) return;
      scroller.scrollTop = top - (scroller.clientHeight - rowHeight) / 2;
      // Update the window now: the scroll event arrives after this frame is painted.
      measure();
    },
    [listTop, measure, rowHeight],
  );

  return {
    start: Math.min(range.start, count),
    end: Math.min(range.end, count),
    reveal,
  };
}

function scrollParent(element: HTMLElement | null) {
  let panel = element?.parentElement ?? null;
  while (panel && !/(auto|scroll)/.test(getComputedStyle(panel).overflowY))
    panel = panel.parentElement;
  return panel;
}
