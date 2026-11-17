import type { Snapshot } from "../types.ts";
import type { Margin } from "./overscan.ts";

/** Drawing backends in fallback order; Canvas2D is the page's own last resort. */
export type Backend = "webgpu" | "webgl2" | "cpu";

/**
 * Simulated WebGPU failures (debug/test only): presents that leave the canvas blank, a
 * render that throws, a real uncaptured validation error.
 */
export type GpuFault = "blank" | "throw" | "error";

/** The frame summary the engine returns instead of its scene (`crates/engine-wasm`). */
export interface EngineSummary {
  projectId: string;
  frameId: string;
  revision: number;
  width: number;
  height: number;
  nodeCount: number;
  sceneVersion: string;
  rendererVersion: string;
  coordinateSpace: string;
  sourceSceneKey: string;
  diagnostics: unknown[];
  fonts: string[];
  assets: { id: string; hash: string; mediaType: string }[];
  /** Union of node bounds `[x, y, width, height]`, frame-local; clicks outside cannot hit. */
  hitArea: [number, number, number, number] | null;
}

export interface FrameRequest {
  frameId: string;
  width: number;
  height: number;
}

export interface Placement {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Camera {
  /** Device px per scene px (zoom × devicePixelRatio). */
  scale: number;
  tx: number;
  ty: number;
  /** Canvas size in device px. */
  width: number;
  height: number;
}

export interface NodeBounds {
  frameId: string;
  id: string;
  kind: string;
  x: number;
  y: number;
  width: number;
  height: number;
  lines: number;
}

export type PageMessage =
  /** Draw with `backend` on a fresh worker canvas; images come back as `drawn`. */
  | {
      type: "attach";
      backend: Backend;
      textMode: "outlines" | "atlas";
      /** Debug: applied to a WebGPU surface before its present probe. */
      fault?: GpuFault;
    }
  | {
      type: "sync";
      generation: number;
      projectId: string;
      revision: number;
      /** Only when the worker does not hold this revision yet. */
      snapshot?: Snapshot;
      frames: FrameRequest[];
      /** Retry: recompute even frames whose content key is unchanged. */
      bypass: boolean;
    }
  /** `seq` increases per layout; the worker answers `drawn` once a render shows it. */
  | { type: "layout"; placements: Placement[]; seq: number }
  | { type: "camera"; camera: Camera }
  /** Fraction of the viewport drawn beyond each edge (`overscan.ts`); 0 = none (default). */
  | { type: "overscan"; ratio: number }
  | { type: "hit"; id: number; x: number; y: number; revision: number }
  | {
      type: "bounds";
      id: number;
      frameId: string;
      revision: number;
      ids: string[];
    }
  | { type: "status"; id: number }
  | { type: "lose-device" }
  | { type: "gpu-fault"; fault: GpuFault };

export type WorkerMessage =
  /** Module loaded; the page may attach a canvas. */
  | { type: "ready"; loadMs: number }
  /** Module or engine bootstrap failed: the page falls back to Canvas2D. */
  | { type: "fatal"; error: string }
  | { type: "backend"; backend: Backend; initMs: number; rebuilt?: boolean }
  /** This backend cannot draw here; the page attaches a fresh canvas for the next one. */
  | { type: "backend-failed"; backend: Backend; error: string }
  | {
      type: "frame";
      generation: number;
      frameId: string;
      revision: number;
      summary?: EngineSummary;
      /**
       * Content unchanged since the shown revision: no recompute happened. `summary` and
       * `missing` then repeat what the engine shows, now at `revision`.
       */
      reused?: boolean;
      missing?: string[];
      error?: string;
    }
  | { type: "synced"; generation: number; error?: string }
  | { type: "rendered"; stats: RenderStats }
  /**
   * One render: `image` shows layout `layout` drawn with `camera` (device px), grown by
   * `margin` device px on each side (overscan). The page displays it and places it for its
   * current camera in the same task.
   */
  | { type: "drawn"; layout: number; camera?: Camera; margin?: Margin; image: ImageBitmap }
  | { type: "reply"; id: number; value: unknown };

export interface RenderStats {
  encodeMs: number;
  submitMs: number;
  frames: number;
  culledFrames: number;
  nodes: number;
  glyphs: number;
  greeked: number;
  culled: number;
  analyticShadows: boolean;
}
