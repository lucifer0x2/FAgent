import { frameLabelMetrics } from "../scene/frameLabels.ts";

export interface OverlayFrame {
  id: string;
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  labelSpace?: number;
  /** No scene yet: a skeleton slot. */
  pending: boolean;
  /** No scene and a failure: the frame's error text on a white slot. */
  error?: string;
}

export interface OverlayBox {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface OverlayInput {
  frames: OverlayFrame[];
  transform: { x: number; y: number; scale: number };
  pixelRatio: number;
  labels: boolean;
  selection?: OverlayBox;
  hover?: OverlayBox;
}

const SELECTION = "#1672e8";

/**
 * The thin Canvas2D layer over the engine canvas: frame labels, skeleton and error slots,
 * hover and selection outlines. Same look as the Konva board; nothing here reaches the GPU.
 */
export function paintOverlay(
  context: CanvasRenderingContext2D,
  { frames, transform, pixelRatio, labels, selection, hover }: OverlayInput,
) {
  const { scale } = transform;
  context.setTransform(1, 0, 0, 1, 0, 0);
  context.clearRect(0, 0, context.canvas.width, context.canvas.height);
  context.setTransform(
    pixelRatio * scale,
    0,
    0,
    pixelRatio * scale,
    pixelRatio * transform.x,
    pixelRatio * transform.y,
  );
  for (const frame of frames) {
    context.save();
    context.translate(frame.x, frame.y);
    if (labels) label(context, frame, scale, pixelRatio);
    if (frame.pending) skeleton(context, frame, scale);
    else if (frame.error) failure(context, frame, scale, pixelRatio);
    context.restore();
  }
  for (const [box, alpha, width] of [
    [hover, 0.55, 1],
    [selection, 1, 1.5],
  ] as const) {
    if (!box) continue;
    context.globalAlpha = alpha;
    context.strokeStyle = SELECTION;
    context.lineWidth = width / scale;
    context.strokeRect(box.x, box.y, box.width, box.height);
    context.globalAlpha = 1;
  }
}

/** 12 CSS px labels, drawn in screen space (a board-space font would be 12/zoom px). */
function label(
  context: CanvasRenderingContext2D,
  frame: OverlayFrame,
  scale: number,
  pixelRatio: number,
) {
  const metrics = frameLabelMetrics(frame.width, scale, frame.labelSpace ?? 32);
  if (!metrics.visible) return;
  const at = context.getTransform();
  context.save();
  context.setTransform(pixelRatio, 0, 0, pixelRatio, at.e, at.f + metrics.y * at.d);
  context.font = "12px Arial, sans-serif";
  context.fillStyle = "#505b6b";
  context.textBaseline = "top";
  context.fillText(ellipsis(context, frame.name, frame.width * scale), 0, 0);
  context.restore();
}

const fitted = new Map<string, string>();

/** Longest prefix + "…" within `width` CSS px at 12px Arial (memoized, binary search). */
function ellipsis(context: CanvasRenderingContext2D, text: string, width: number) {
  const key = `${Math.floor(width)}|${text}`;
  const cached = fitted.get(key);
  if (cached !== undefined) return cached;
  let result = text;
  if (context.measureText(text).width > width) {
    let low = 0,
      high = text.length;
    while (low < high) {
      const middle = Math.ceil((low + high) / 2);
      if (context.measureText(`${text.slice(0, middle)}…`).width <= width) low = middle;
      else high = middle - 1;
    }
    result = `${text.slice(0, low)}…`;
  }
  if (fitted.size > 2000) fitted.clear();
  fitted.set(key, result);
  return result;
}

function skeleton(context: CanvasRenderingContext2D, frame: OverlayFrame, scale: number) {
  context.fillStyle = "#f5f7fa";
  context.fillRect(0, 0, frame.width, frame.height);
  context.strokeStyle = "#d9dfe8";
  context.lineWidth = 1 / scale;
  context.strokeRect(0, 0, frame.width, frame.height);
  context.fillStyle = "#e4e9f0";
  [0.65, 0.85, 0.5].forEach((width, index) =>
    context.fillRect(
      frame.width * 0.1,
      frame.height * (0.12 + index * 0.1),
      frame.width * width,
      frame.height * 0.035,
    ),
  );
}

function failure(
  context: CanvasRenderingContext2D,
  frame: OverlayFrame,
  scale: number,
  pixelRatio: number,
) {
  context.save();
  // Canvas shadows are in device px; the Konva backdrop scales them with the stage.
  context.shadowColor = "rgba(29, 36, 51, 0.12)";
  context.shadowBlur = 20 * scale * pixelRatio;
  context.shadowOffsetY = 5 * scale * pixelRatio;
  context.fillStyle = "white";
  context.fillRect(0, 0, frame.width, frame.height);
  context.restore();
  context.fillStyle = "#a73429";
  context.font = "16px Arial, sans-serif";
  context.textBaseline = "top";
  const lines = wrap(
    context,
    `Frame unavailable\n${frame.error}`,
    Math.max(1, frame.width - 48),
  );
  lines.forEach((line, index) => context.fillText(line, 24, 24 + index * 24));
}

function wrap(context: CanvasRenderingContext2D, text: string, width: number) {
  const lines: string[] = [];
  for (const paragraph of text.split("\n")) {
    let line = "";
    for (const word of paragraph.split(" ")) {
      const next = line ? `${line} ${word}` : word;
      if (line && context.measureText(next).width > width) {
        lines.push(line);
        line = word;
      } else line = next;
    }
    lines.push(line);
  }
  return lines;
}
