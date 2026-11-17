import { memo, useEffect, useMemo, useRef, useState } from "react";
import { Stage, Layer } from "react-konva";
import type Konva from "konva";
import { BoardPaint } from "../paint/boardPaint";
import { boardHitTest } from "../scene/boardHitTest";
import { CanvasScenes } from "./CanvasScenes";
import { CanvasChrome } from "./CanvasChrome";
import { CanvasMessage } from "./CanvasMessage";
import { EngineCanvasView } from "./EngineCanvasView";
import { useCanvasTransform } from "../hooks/useCanvasTransform";
import { useCanvasGestures } from "../hooks/useCanvasGestures";
import { panDragStep, startPanDrag, type PanDrag } from "../hooks/canvasPointers";
import type { SceneView } from "../hooks/useScene";
import { useNodeActions } from "./NodeActionsProvider";
import { useCanvasFrames } from "../hooks/useCanvasFrames";

// A click that moves further than this is a pan, not a selection.
const CLICK_SLOP = 5;

const KonvaCanvasView = memo(function KonvaCanvasView({
  view,
  selectedId,
  select,
  retry,
  frameName,
}: {
  view: SceneView;
  selectedId: string;
  select: (id: string) => void;
  retry: () => void;
  frameName: string;
}) {
  const stage = useRef<Konva.Stage>(null);
  const layer = useRef<Konva.Layer>(null);
  // Frame bitmaps of the All board share zoom-settle and memory state; one per canvas.
  const [board] = useState(
    () => new BoardPaint(() => layer.current?.batchDraw()),
  );
  useEffect(() => () => board.dispose(), [board]);
  const actions = useNodeActions();
  const { frames, boardLayout, content } = useCanvasFrames(view, frameName);
  const {
    host,
    size,
    viewport,
    transform,
    zoom,
    fit,
    panBy,
    handleKeyDown,
    STEP,
  } = useCanvasTransform(content, view.cameraKey, view.frames ? 0.0001 : 0.05);
  // A one-pointer pan on the host, as on the engine canvas: each move pans the latest camera
  // by its delta, so wheel zoom during the drag composes with it. (Konva's stage drag kept an
  // offset from the drag start and undid any zoom made while dragging.)
  const drag = useRef<PanDrag | null>(null);
  // Wheel zoom, and pinch/two-finger pan; a second finger ends the one-pointer pan and tap.
  const { gesturing } = useCanvasGestures(host, {
    zoom,
    start: () => (drag.current = null),
  });
  const placements = useMemo(
    () => view.frames
      ? boardLayout.frames
      : frames.map((frame) => ({ ...frame, x: 0, y: 0 })),
    [view.frames, boardLayout, frames],
  );
  const findNode = (x: number, y: number) =>
    boardHitTest(
      frames,
      placements,
      (x - transform.x) / transform.scale,
      (y - transform.y) / transform.scale,
      view.requestedRevision,
    );
  const pixelRatio = window.devicePixelRatio || 1;
  // Whole device pixels, so panning does not shift the painted scene by subpixel amounts.
  const snap = (value: number) => Math.round(value * pixelRatio) / pixelRatio;
  const interactive = (target: EventTarget) =>
    !(target instanceof Element && target.closest("button, a, input, textarea, [role=button]"));
  const hostPoint = (event: { clientX: number; clientY: number }) => {
    const rect = host.current!.getBoundingClientRect();
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  };

  return (
    <main
      className="canvas-pane"
      aria-label="Design canvas"
      aria-busy={view.status === "pending"}
    >
      <div
        className="canvas-host"
        ref={host}
        tabIndex={0}
        aria-label="Canvas. Drag to pan. Plus and minus to zoom. F to fit. Select layers in the layers panel."
        onKeyDown={handleKeyDown}
        onPointerDown={(event) => {
          if (event.button !== 0 || !interactive(event.target) || gesturing()) return;
          drag.current = startPanDrag(hostPoint(event));
          event.currentTarget.setPointerCapture?.(event.pointerId);
        }}
        onPointerMove={(event) => {
          // A mouse whose button was released unseen (outside the window) is no longer dragging.
          if (drag.current && event.pointerType === "mouse" && !(event.buttons & 1))
            drag.current = null;
          const delta = drag.current && panDragStep(drag.current, hostPoint(event), CLICK_SLOP);
          if (delta) panBy(delta);
        }}
        onPointerUp={(event) => {
          const start = drag.current;
          drag.current = null;
          if (event.button !== 0 || !start || start.moved || !interactive(event.target)) return;
          // A click selects the node under it; the background (or no scene yet) deselects.
          const point = hostPoint(event);
          const found = frames.length ? findNode(point.x, point.y) : undefined;
          select(found?.id ?? "");
          if (!found && document.activeElement instanceof HTMLElement)
            document.activeElement.blur();
        }}
        onPointerCancel={() => (drag.current = null)}
        onLostPointerCapture={(event) => {
          // Only the host losing its own capture; a child handing its touch capture over is not.
          if (event.target === event.currentTarget) drag.current = null;
        }}
      >
        {frames.length > 0 && (
          <Stage
            ref={stage}
            width={size.width}
            height={size.height}
            x={snap(transform.x)}
            y={snap(transform.y)}
            scaleX={transform.scale}
            scaleY={transform.scale}
            onContextMenu={(event) => {
              const point = stage.current?.getPointerPosition();
              if (!point) return;
              const found = findNode(point.x, point.y);
              if (found && actions) {
                event.evt.preventDefault();
                actions.open(found.id, event.evt.clientX, event.evt.clientY);
              }
            }}
          >
            <Layer ref={layer} imageSmoothingEnabled>
              <CanvasScenes
                frames={frames}
                placements={placements}
                scale={transform.scale}
                selectedId={selectedId}
                labels={Boolean(view.frames)}
                board={view.frames ? board : undefined}
              />
            </Layer>
          </Stage>
        )}
        <CanvasMessage view={view} retry={retry} />
      </div>
      <CanvasChrome
        frameName={frameName}
        viewport={
          view.frames && viewport
            ? {
                width: Math.round(viewport.width),
                height: Math.round(viewport.height),
              }
            : viewport
        }
        scale={transform.scale}
        enabled={frames.length > 0}
        zoom={zoom}
        fit={fit}
        step={STEP}
      />
    </main>
  );
});

// Engine props are a superset (`sections`: the All pages board, engine only).
type CanvasProps = Parameters<typeof EngineCanvasView>[0];

/** Konva/Canvas2D by default; the Vello engine canvas when the view comes from the engine. */
export const CanvasView = memo(function CanvasView(props: CanvasProps) {
  return props.view.engine ? (
    <EngineCanvasView {...props} />
  ) : (
    <KonvaCanvasView {...props} />
  );
});
