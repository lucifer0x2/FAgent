import { useMemo } from "react";
import type { ComputedNode } from "../components/Inspector";
import { ENGINE_FALLBACK, backendLabel } from "../engine/client";
import { rendererChoice } from "../engine/flag";
import {
  useEngineInspection,
  useEngineStart,
  useEngineState,
} from "../engine/useEngine";
import { useEngineScenes } from "../engine/useEngineScenes";
import { ALL_PAGES, frameDimensions } from "../frameScope";
import { usePageScenes } from "../hooks/usePageScenes";
import { useScene, type SceneView } from "../hooks/useScene";
import { currentScene, shownRevision } from "../scene/staleFrames";
import { nodeName } from "../state";
import type { DesignNode, SceneRequest, Snapshot } from "../types";

interface Input {
  snapshot: Snapshot | null;
  pageId: string;
  frames: DesignNode[];
  frame?: DesignNode;
  selectedFrame?: DesignNode;
  nodeId: string;
  all: boolean;
  preset: string;
  viewport: { width: number; height: number };
  request: SceneRequest | null;
  retryKey: number;
}

/**
 * The canvas view for this session's renderer: the Vello engine Worker (default) or
 * Canvas2D (`renderer=canvas2d` or server scenes; scenes on the main thread). An engine that cannot start
 * falls back to Canvas2D for the rest of the session, with a visible diagnostic.
 */
export function useWorkspaceView(input: Input) {
  const { snapshot, pageId, frames, frame, all, preset, viewport, request, retryKey } =
    input;
  const choice = useMemo(() => rendererChoice(), []);
  useEngineStart(choice);
  const engine = useEngineState();
  const vello = choice.renderer === "vello" && engine.phase !== "failed";

  const singleView = useScene(vello ? null : request, retryKey, preset, snapshot);
  // Canvas2D page boards load one page through the server; never the All pages scope.
  const pageView = usePageScenes(
    all && !vello && pageId !== ALL_PAGES ? snapshot : null,
    pageId,
    frames,
    preset,
    viewport,
    retryKey,
  );
  const { width, height } = viewport;
  const targets = useMemo(
    () =>
      all
        ? frames.map((entry) => ({
            id: entry.id,
            name: nodeName(entry),
            ...frameDimensions(entry),
            ...(width ? { width, height } : {}),
          }))
        : request && frame
          ? [
              {
                id: request.frameId,
                name: nodeName(frame),
                width: request.width,
                height: request.height,
                hugHeight: request.hugHeight,
              },
            ]
          : [],
    [all, frames, width, height, request, frame],
  );
  const scope = snapshot
    ? JSON.stringify(
        all
          ? [snapshot.projectId, pageId, "all", preset]
          : [snapshot.projectId, frame?.id ?? "", preset],
      )
    : "";
  const engineView = useEngineScenes(vello, snapshot, targets, scope, retryKey, all);
  const canvasView = all ? pageView : singleView;
  const view: SceneView = useMemo(() => {
    if (vello)
      return {
        ...engineView,
        diagnostics: [...engine.notes, ...engineView.diagnostics],
        backend: backendLabel(engine.backend),
      };
    if (choice.renderer !== "vello") return canvasView;
    return {
      ...canvasView,
      diagnostics: [
        `${ENGINE_FALLBACK}: ${engine.error ?? "unknown error"}`,
        ...canvasView.diagnostics,
      ],
    };
  }, [vello, engineView, engine, choice.renderer, canvasView]);

  const inspection = useEngineInspection(
    vello ? view : { status: "empty", missing: [], diagnostics: [] },
    input.selectedFrame?.id,
    input.nodeId,
    snapshot?.revision,
  );
  // Canvas2D: computed geometry only from a scene answering exactly the current snapshot.
  const inspected = vello
    ? undefined
    : all
      ? currentScene(
          view.frames?.find((entry) => entry.id === input.selectedFrame?.id),
          snapshot?.revision,
        )
      : shownRevision(view) === snapshot?.revision
        ? view.scene
        : undefined;
  const scene =
    inspected?.provenance.projectId === snapshot?.projectId ? inspected : undefined;
  const node = scene?.nodes.find((entry) => entry.id === input.nodeId);
  const computed: ComputedNode | undefined = vello
    ? inspection.computed
    : node && { bounds: node.bounds, kind: node.kind, lines: node.lines.length };
  const source = vello
    ? inspection.source
    : scene && {
        rendererVersion: scene.provenance.rendererVersion,
        coordinateSpace: scene.coordinateSpace,
      };
  return { view, computed, source };
}
