import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { engineClient } from "../engine/client";
import { paintOverlay, type OverlayBox } from "../engine/overlay";
import type { Camera, NodeBounds } from "../engine/protocol";
import { stalePlacement } from "../engine/staleCamera";
import { engineView, useEngineState } from "../engine/useEngine";
import { panDragStep, startPanDrag, type PanDrag } from "../hooks/canvasPointers";
import { useCanvasGestures } from "../hooks/useCanvasGestures";
import { useCanvasTransform } from "../hooks/useCanvasTransform";
import type { SceneView } from "../hooks/useScene";
import { fitFrameBoard, type FrameSize } from "../scene/frameLayout";
import { fitPageBoard, type BoardSection, type SectionPlacement } from "../scene/pageBoard";
import { paintSectionBackdrops, paintSectionTitles, type SectionTab } from "../engine/sections";
import { CanvasChrome } from "./CanvasChrome";
import { CanvasMessage } from "./CanvasMessage";
import { useNodeActions } from "./NodeActionsProvider";

// A click that moves further than this is a pan, not a selection.
const CLICK_SLOP = 5;
type BoardLayout = ReturnType<typeof fitPageBoard>;

/**
 * `renderer=vello`: the engine Worker draws the scene and posts each image to the canvas
 * here; this component only sends the camera and layout, places the latest image for the
 * current camera, hit-tests through the engine and paints a thin overlay (labels,
 * skeletons, hover and selection) on the main thread.
 */
export const EngineCanvasView = memo(function EngineCanvasView({
  view,
  selectedId,
  select,
  retry,
  frameName,
  sections,
}: {
  view: SceneView;
  selectedId: string;
  select: (id: string) => void;
  retry: () => void;
  frameName: string;
  /** All pages board: frames grouped and titled by page. */
  sections?: BoardSection[];
}) {
  const engine = useEngineState();
  const actions = useNodeActions();
  const board = view.engine?.board ?? false;
  const frames = useMemo(() => view.engine?.frames ?? [], [view.engine]);
  const single = board ? undefined : frames[0];
  const geometry = JSON.stringify(
    frames.map(({ id, width, height }) => ({ id, width, height })),
  );
  const sectionKey = sections ? JSON.stringify(sections) : "";
  // Board positions depend only on frame sizes and pages, never on the pane size: closing a
  // drawer or resizing the window moves nothing on the board.
  const boardLayout = useMemo((): BoardLayout => {
    const sizes: FrameSize[] = JSON.parse(geometry);
    const groups: BoardSection[] | undefined = sectionKey ? JSON.parse(sectionKey) : undefined;
    return groups
      ? fitPageBoard(sizes, groups)
      : { ...fitFrameBoard(sizes), sections: [] };
  }, [geometry, sectionKey]);
  const singleSize = single?.summary
    ? { width: single.summary.width, height: single.summary.height }
    : undefined;
  const singleWidth = singleSize?.width,
    singleHeight = singleSize?.height;
  const content = useMemo(
    () =>
      board
        ? frames.length
          ? boardLayout
          : undefined
        : singleWidth !== undefined && singleHeight !== undefined
          ? { width: singleWidth, height: singleHeight }
          : undefined,
    [board, frames.length, boardLayout, singleWidth, singleHeight],
  );
  const { host, size, viewport, transform, zoom, fit, fitTo, panBy, handleKeyDown, STEP } =
    useCanvasTransform(content, view.cameraKey, board ? 0.0001 : 0.05);
  const placements = useMemo(
    () =>
      board
        ? boardLayout.frames
        : single
          ? [
              {
                id: single.id,
                x: 0,
                y: 0,
                width: singleWidth ?? single.width,
                height: singleHeight ?? single.height,
              },
            ]
          : [],
    [board, boardLayout, single, singleWidth, singleHeight],
  );
  const placedSections: SectionPlacement[] = board ? boardLayout.sections : [];
  const pixelRatio = window.devicePixelRatio || 1;
  // Whole device pixels, so panning does not shift the painted scene by subpixel amounts.
  const tx = Math.round(transform.x * pixelRatio),
    ty = Math.round(transform.y * pixelRatio);

  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    if (canvas.current && (engine.phase === "booting" || engine.phase === "attaching"))
      engineClient.attach(canvas.current);
  }, [engine.canvasKey, engine.phase]);
  // Before paint, so the canvas hides in the same frame the new page's skeletons appear.
  useLayoutEffect(() => {
    engineClient.setLayout(
      placements.map(({ id, x, y, width, height }) => ({ id, x, y, width, height })),
    );
  }, [placements, engine.canvasKey]);
  // The engine canvas shows the worker's latest image, drawn with an older camera while
  // the worker computes a frame; a CSS transform keeps it aligned with this camera (and
  // the overlay) until the next image arrives. With overscan on (FAGENT_VIEWER_OVERSCAN) the
  // image sits at −margin and a pan slides in drawn pixels (the host clips the rest).
  const camera = useRef<Camera | undefined>(undefined);
  const placeCanvas = useCallback(() => {
    const element = canvas.current;
    if (!element || !camera.current) return;
    const placement = stalePlacement(
      engineClient.presented(),
      camera.current,
      pixelRatio,
      engineClient.presentedOverscan(),
    );
    element.style.transform = placement.transform;
    element.style.width = placement.width;
    element.style.height = placement.height;
  }, [pixelRatio]);
  useEffect(() => engineClient.onPresented(placeCanvas), [placeCanvas]);
  // Before paint, with the overlay, so the camera reaches the worker without a frame's delay.
  useLayoutEffect(() => {
    if (!size.width || !size.height) return;
    camera.current = {
      scale: transform.scale * pixelRatio,
      tx,
      ty,
      width: Math.round(size.width * pixelRatio),
      height: Math.round(size.height * pixelRatio),
    };
    engineClient.setCamera(camera.current);
    placeCanvas();
  }, [transform.scale, tx, ty, size.width, size.height, pixelRatio, engine.canvasKey, placeCanvas]);

  // Board point of a host-relative pointer position, in the camera the engine draws with.
  const toBoard = (x: number, y: number) => ({
    x: (x - tx / pixelRatio) / transform.scale,
    y: (y - ty / pixelRatio) / transform.scale,
  });
  const offset = (frameId: string) => placements.find((p) => p.id === frameId);
  const toBox = (bounds: NodeBounds | null | undefined): OverlayBox | undefined => {
    const at = bounds && offset(bounds.frameId);
    return at && bounds
      ? { x: at.x + bounds.x, y: at.y + bounds.y, width: bounds.width, height: bounds.height }
      : undefined;
  };
  const revision = view.requestedRevision ?? -1;
  const boardPoint = (event: { clientX: number; clientY: number }) => {
    const rect = host.current!.getBoundingClientRect();
    return toBoard(event.clientX - rect.left, event.clientY - rect.top);
  };
  /** Whether a current frame has a node near `point`; if not, it is background at once. */
  const hittable = (point: { x: number; y: number }) =>
    frames.some((frame) => {
      const area = frame.summary?.hitArea;
      const at = offset(frame.id);
      if (!area || !at || frame.stale || frame.revision !== revision) return false;
      const [x, y, width, height] = area;
      return (
        point.x >= at.x + x &&
        point.y >= at.y + y &&
        point.x <= at.x + x + width &&
        point.y <= at.y + y + height
      );
    });
  const hit = (point: { x: number; y: number }) =>
    hittable(point)
      ? engineClient.hitTest(point.x, point.y, revision)
      : Promise.resolve(null);
  /** The page whose title tab is under a host-relative point. */
  const sectionAt = (x: number, y: number) => {
    const tab = tabs.current.find(
      (t) => x >= t.x && y >= t.y && x <= t.x + t.width && y <= t.y + t.height,
    );
    return tab && placedSections.find((s) => s.id === tab.id);
  };
  const hostPoint = (event: { clientX: number; clientY: number }) => {
    const rect = host.current!.getBoundingClientRect();
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  };
  const deselect = () => {
    select("");
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
  };

  const [selection, setSelection] = useState<NodeBounds | null>(null);
  const shown = frames.map((f) => `${f.id}:${f.revision}:${f.stale ?? false}`).join();
  useEffect(() => {
    let live = true;
    if (!selectedId) setSelection(null);
    else
      void engineClient.bounds("", -1, [selectedId]).then((found) => {
        if (live) setSelection(found[0] ?? null);
      });
    return () => {
      live = false;
    };
  }, [selectedId, shown]);
  const [hover, setHover] = useState<NodeBounds | null>(null);
  const hoverRequest = useRef(0);

  const overlay = useRef<HTMLCanvasElement>(null);
  const underlay = useRef<HTMLCanvasElement>(null);
  const tabs = useRef<SectionTab[]>([]);
  useLayoutEffect(() => {
    const element = overlay.current;
    if (!element) return;
    const width = Math.round(size.width * pixelRatio),
      height = Math.round(size.height * pixelRatio);
    for (const surface of [element, underlay.current]) {
      if (!surface) continue;
      if (surface.width !== width) surface.width = width;
      if (surface.height !== height) surface.height = height;
    }
    const context = element.getContext("2d");
    if (!context) return;
    const camera = { x: tx / pixelRatio, y: ty / pixelRatio, scale: transform.scale };
    const below = underlay.current?.getContext("2d");
    if (below) paintSectionBackdrops(below, placedSections, camera, pixelRatio);
    engineView.camera = camera;
    engineView.selection = toBox(selection);
    paintOverlay(context, {
      frames: placements.map((p) => {
        const frame = frames.find((f) => f.id === p.id);
        return {
          ...p,
          name: frame?.name ?? "",
          labelSpace: "labelSpace" in p ? (p.labelSpace as number) : undefined,
          pending: board && !frame?.summary && !frame?.error,
          error: frame?.summary ? undefined : frame?.error,
        };
      }),
      transform: camera,
      pixelRatio,
      labels: board,
      selection: engineView.selection,
      hover: hover?.id !== selection?.id ? toBox(hover) : undefined,
    });
    tabs.current = paintSectionTitles(context, placedSections, camera, pixelRatio);
  });

  // A one-pointer pan: each move pans the latest camera by its delta, so wheel zoom during
  // the drag (still anchored at the cursor) composes with it instead of being overwritten.
  const drag = useRef<PanDrag | null>(null);
  // Wheel zoom, and pinch/two-finger pan; a second finger ends this view's pan and tap.
  const { gesturing } = useCanvasGestures(host, {
    zoom,
    start: () => {
      drag.current = null;
      hoverRequest.current++;
      setHover(null);
    },
  });
  const interactive = (target: EventTarget) =>
    !(target instanceof Element && target.closest("button, a, input, textarea, [role=button]"));

  return (
    <main className="canvas-pane" aria-label="Design canvas" aria-busy={view.status === "pending"}>
      <div
        className="canvas-host"
        ref={host}
        tabIndex={0}
        data-renderer="vello"
        data-backend={engine.backend ?? ""}
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
          if (drag.current) {
            const delta = panDragStep(drag.current, hostPoint(event), CLICK_SLOP);
            if (delta) panBy(delta);
            return;
          }
          if (gesturing()) return;
          if (placedSections.length) {
            const at = hostPoint(event);
            event.currentTarget.style.cursor = sectionAt(at.x, at.y) ? "pointer" : "";
          }
          const request = ++hoverRequest.current;
          void hit(boardPoint(event)).then((found) => {
            if (request === hoverRequest.current)
              setHover((old) => (old?.id === found?.id && old?.frameId === found?.frameId ? old : found));
          });
        }}
        onPointerLeave={() => {
          hoverRequest.current++;
          setHover(null);
        }}
        onPointerUp={(event) => {
          const start = drag.current;
          drag.current = null;
          if (event.button !== 0 || !start || start.moved || !interactive(event.target)) return;
          if (placedSections.length) {
            const at = hostPoint(event);
            const section = sectionAt(at.x, at.y);
            if (section) return fitTo(section.focus);
          }
          const point = boardPoint(event);
          // Background clicks deselect synchronously, as on the Konva canvas.
          if (!hittable(point)) return deselect();
          void hit(point).then((found) => (found ? select(found.id) : deselect()));
        }}
        onPointerCancel={() => (drag.current = null)}
        onLostPointerCapture={(event) => {
          // Only the host losing its own capture; a child handing its touch capture over is not.
          if (event.target === event.currentTarget) drag.current = null;
        }}
        onContextMenu={(event) => {
          if (!interactive(event.target) || !actions) return;
          const point = boardPoint(event);
          // Over the background the browser's own menu stays, as on the Konva canvas.
          if (!hittable(point)) return;
          event.preventDefault();
          const { clientX, clientY } = event;
          void hit(point).then((found) => {
            if (found) actions.open(found.id, clientX, clientY);
          });
        }}
      >
        <canvas ref={underlay} className="engine-underlay" aria-hidden="true" />
        <canvas
          key={engine.canvasKey}
          ref={canvas}
          className="engine-canvas"
          aria-hidden="true"
          style={engine.hideUntilLayout !== undefined ? { visibility: "hidden" } : undefined}
        />
        <canvas ref={overlay} className="engine-overlay" aria-hidden="true" />
        <CanvasMessage view={view} retry={retry} />
      </div>
      <CanvasChrome
        frameName={frameName}
        viewport={
          board && viewport
            ? { width: Math.round(viewport.width), height: Math.round(viewport.height) }
            : viewport
        }
        scale={transform.scale}
        enabled={board ? frames.length > 0 : Boolean(single?.summary)}
        zoom={zoom}
        fit={fit}
        step={STEP}
      />
    </main>
  );
});
