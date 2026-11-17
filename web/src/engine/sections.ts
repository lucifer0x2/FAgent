import type { SectionPlacement } from "../scene/pageBoard.ts";

type Camera = { x: number; y: number; scale: number };

/** A drawn title tab, in host CSS px (for clicks). */
export interface SectionTab {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

// One accent per page, cycled, so neighbouring pages read apart at any zoom.
const ACCENTS = ["#1672e8", "#0f9d76", "#d9730d", "#8e4ad8", "#d6336c", "#0b8ea0"];
const TAB = { height: 24, padding: 9, dot: 8, above: 6, min: 150 };

export const sectionAccent = (index: number) => ACCENTS[index % ACCENTS.length];

/** Screen rectangle of a section for this camera (host CSS px). */
function onScreen(section: SectionPlacement, camera: Camera) {
  return {
    left: camera.x + section.x * camera.scale,
    top: camera.y + section.y * camera.scale,
    width: section.width * camera.scale,
    height: section.height * camera.scale,
  };
}

/** Tinted page areas, painted under the engine image (the engine clears to transparent). */
export function paintSectionBackdrops(
  context: CanvasRenderingContext2D,
  sections: SectionPlacement[],
  camera: Camera,
  pixelRatio: number,
) {
  context.setTransform(1, 0, 0, 1, 0, 0);
  context.clearRect(0, 0, context.canvas.width, context.canvas.height);
  context.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
  const view = { width: context.canvas.width / pixelRatio, height: context.canvas.height / pixelRatio };
  for (const section of sections) {
    const box = onScreen(section, camera);
    if (
      box.left > view.width ||
      box.top > view.height ||
      box.left + box.width < 0 ||
      box.top + box.height < 0
    )
      continue;
    const accent = sectionAccent(section.index);
    context.beginPath();
    // Square corners: zoomed out, a page is small and a rounded outline reads as a blob.
    context.rect(box.left, box.top, box.width, box.height);
    context.globalAlpha = 0.07;
    context.fillStyle = accent;
    context.fill();
    context.globalAlpha = 0.45;
    context.strokeStyle = accent;
    context.lineWidth = 1;
    context.stroke();
    context.globalAlpha = 1;
  }
}

/**
 * Page title tabs at a constant 13 CSS px, above their page (not over its content). When
 * the page's top is scrolled off screen the tab stays pinned to the top of the canvas.
 */
export function paintSectionTitles(
  context: CanvasRenderingContext2D,
  sections: SectionPlacement[],
  camera: Camera,
  pixelRatio: number,
): SectionTab[] {
  context.save();
  context.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
  const view = { width: context.canvas.width / pixelRatio, height: context.canvas.height / pixelRatio };
  const tabs: SectionTab[] = [];
  const boxes = sections.map((section) => onScreen(section, camera));
  const tabY = boxes.map((box) =>
    Math.max(
      box.top - TAB.above - TAB.height,
      Math.min(8, box.top + box.height - TAB.height - TAB.above),
    ),
  );
  for (const [i, section] of sections.entries()) {
    const box = boxes[i];
    if (box.width < 16 || box.height < 16) continue;
    const right = box.left + box.width,
      bottom = box.top + box.height;
    const y = tabY[i];
    if (box.left > view.width || y > view.height || right < 0 || bottom < 0) continue;
    const x = Math.max(box.left, Math.min(8, right - TAB.height));
    // A narrow page (zoomed out) may carry a tab wider than itself, up to the next tab.
    let limit = Math.max(right, x + TAB.min);
    for (const [j, other] of boxes.entries())
      if (j !== i && other.left > box.left && Math.abs(tabY[j] - y) < TAB.height)
        limit = Math.min(limit, other.left - 6);
    const available = limit - x;
    if (available < TAB.height) continue;
    const count = `${section.frames} ${section.frames === 1 ? "frame" : "frames"}`;
    const room = available - 2 * TAB.padding - TAB.dot - 8;
    context.font = "12px Arial, sans-serif";
    const countWidth = context.measureText(count).width;
    context.font = "600 13px Arial, sans-serif";
    const full = context.measureText(section.name).width;
    const showCount = full + 8 + countWidth <= room;
    const name = room >= 24 ? fit(context, section.name, room) : "";
    const nameWidth = name ? context.measureText(name).width : 0;
    const width = name
      ? TAB.padding * 2 + TAB.dot + 8 + nameWidth + (showCount ? 8 + countWidth : 0)
      : TAB.height;
    context.shadowColor = "rgba(29, 36, 51, 0.14)";
    context.shadowBlur = 6;
    context.shadowOffsetY = 1;
    context.fillStyle = "#ffffff";
    context.beginPath();
    context.roundRect(x, y, width, TAB.height, TAB.height / 2);
    context.fill();
    context.shadowColor = "transparent";
    const accent = sectionAccent(section.index);
    context.fillStyle = accent;
    context.beginPath();
    const dotX = name ? x + TAB.padding + TAB.dot / 2 : x + TAB.height / 2;
    context.arc(dotX, y + TAB.height / 2, TAB.dot / 2, 0, Math.PI * 2);
    context.fill();
    context.textBaseline = "middle";
    const textX = x + TAB.padding + TAB.dot + 8;
    context.font = "600 13px Arial, sans-serif";
    context.fillStyle = "#1d2433";
    context.fillText(name, textX, y + TAB.height / 2 + 0.5);
    if (showCount) {
      context.font = "12px Arial, sans-serif";
      context.fillStyle = "#6b7686";
      context.fillText(count, textX + nameWidth + 8, y + TAB.height / 2 + 0.5);
    }
    tabs.push({ id: section.id, x, y, width, height: TAB.height });
  }
  context.restore();
  return tabs;
}

/** Longest prefix + "…" of `text` within `width` px in the context's current font. */
function fit(context: CanvasRenderingContext2D, text: string, width: number) {
  if (context.measureText(text).width <= width) return text;
  let low = 0,
    high = text.length;
  while (low < high) {
    const middle = Math.ceil((low + high) / 2);
    if (context.measureText(`${text.slice(0, middle)}…`).width <= width) low = middle;
    else high = middle - 1;
  }
  return `${text.slice(0, low)}…`;
}
