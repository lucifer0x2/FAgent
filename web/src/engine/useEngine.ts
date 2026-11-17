import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import type { SceneView } from "../hooks/useScene";
import { useViewerOverscan } from "../hooks/useViewerAccess";
import { engineClient, type EngineState } from "./client";
import { rendererChoice, type RendererChoice } from "./flag";
import type { GpuFault, NodeBounds } from "./protocol";

export function useEngineState(): EngineState {
  return useSyncExternalStore(engineClient.subscribe, engineClient.getState);
}

const engineFailed = () => engineClient.getState().phase === "failed";

/**
 * The All pages board is drawn only by the Vello engine (Canvas2D boards load one page
 * through the server); false once the engine has fallen back to Canvas2D.
 */
export function useAllPagesBoard(): boolean {
  const vello = useMemo(() => rendererChoice().renderer === "vello", []);
  const failed = useSyncExternalStore(engineClient.subscribe, engineFailed);
  return vello && !failed;
}

/** What the engine canvas shows, for diagnostics and browser tests (CSS px, board units). */
export const engineView: {
  /** Host-relative camera: board point p is drawn at `p * scale + (x, y)`. */
  camera?: { x: number; y: number; scale: number };
  /** Selection outline in board coordinates, when one is drawn. */
  selection?: { x: number; y: number; width: number; height: number };
} = {};

/**
 * Starts the engine Worker once when this session chose the Vello renderer, and exposes
 * `window.fagentEngine` (status, state, camera, selection; `loseDevice` and `gpuFault` are test hooks).
 * The listener's overscan ratio (`/api/v1/viewer`) reaches the worker whenever it is known.
 */
export function useEngineStart(choice: RendererChoice) {
  const overscan = useViewerOverscan();
  useEffect(() => {
    if (choice.renderer === "vello") engineClient.setOverscan(overscan);
  }, [choice, overscan]);
  useEffect(() => {
    if (choice.renderer !== "vello") return;
    engineClient.start(choice.backend, choice.textMode, choice.gpuFault);
    (window as unknown as { fagentEngine?: unknown }).fagentEngine = {
      status: () => engineClient.status(),
      hitTest: (x: number, y: number, revision: number) =>
        engineClient.hitTest(x, y, revision),
      state: () => engineClient.getState(),
      camera: () => engineView.camera,
      selection: () => engineView.selection,
      loseDevice: () => engineClient.loseDevice(),
      gpuFault: (fault: GpuFault) => engineClient.gpuFault(fault),
    };
  }, [choice]);
}

export interface Inspection {
  computed?: {
    bounds: { x: number; y: number; width: number; height: number };
    kind: string;
    lines: number;
  };
  source?: { rendererVersion: string; coordinateSpace: string };
}

/**
 * Inspector geometry from the engine: only from a frame whose shown scene answers exactly
 * `revision` (never a stale one), as the Canvas2D path uses `currentScene`.
 */
export function useEngineInspection(
  view: SceneView,
  frameId: string | undefined,
  nodeId: string | undefined,
  revision: number | undefined,
): Inspection {
  const frame = view.engine?.frames.find((entry) =>
    view.engine?.board ? entry.id === frameId : true,
  );
  const current =
    frame?.summary && !frame.stale && revision !== undefined && frame.revision === revision
      ? frame
      : undefined;
  const [found, setFound] = useState<{ key: string; value?: NodeBounds }>({ key: "" });
  const key = current && nodeId ? JSON.stringify([current.id, revision, nodeId]) : "";
  useEffect(() => {
    if (!key || !current || !nodeId || revision === undefined) return;
    let live = true;
    void engineClient.bounds(current.id, revision, [nodeId]).then((bounds) => {
      if (live) setFound({ key, value: bounds[0] });
    });
    return () => {
      live = false;
    };
  }, [key, current, nodeId, revision]);
  if (!current?.summary) return {};
  const source = {
    rendererVersion: current.summary.rendererVersion,
    coordinateSpace: current.summary.coordinateSpace,
  };
  const value = found.key === key ? found.value : undefined;
  return {
    source,
    computed: value && {
      // Key order of the scene JSON (sorted), so the Inspector lists them alike.
      bounds: { height: value.height, width: value.width, x: value.x, y: value.y },
      kind: value.kind,
      lines: value.lines,
    },
  };
}
