import type { Backend, GpuFault } from "./protocol.ts";

export const RENDERER_STORAGE_KEY = "fagent.renderer";
const SCENE_STORAGE_KEY = "fagent.scene";

export interface RendererChoice {
  renderer: "canvas2d" | "vello";
  /** First backend to try; later ones follow `nextBackend`. */
  backend: Backend;
  /** Experimental vello_gpu glyph atlas instead of outlines (`?glyphAtlas=1`). */
  textMode: "outlines" | "atlas";
  /** Debug (`?engineDebug=1&gpuFault=blank|throw|error`): simulated WebGPU failure at attach. */
  gpuFault?: GpuFault;
}

const FAULTS: GpuFault[] = ["blank", "throw", "error"];

const BACKENDS: Record<string, Backend> = {
  vello: "webgpu",
  webgpu: "webgpu",
  webgl2: "webgl2",
  cpu: "cpu",
};

/**
 * The Vello engine (WebGPU first) is the default. `?renderer=canvas2d` (or `fagent.renderer`
 * in local storage) keeps the Canvas2D painter; so does choosing server scenes
 * (`?scene=server` or `fagent.scene`), which only the Canvas2D path consumes.
 * `webgl2`/`cpu` start further down the fallback chain (testing). The query beats storage;
 * an explicit renderer beats the scene choice at the same level; unknown values are ignored.
 */
export function rendererChoice(
  search = globalThis.location?.search ?? "",
  storage?: Pick<Storage, "getItem">,
): RendererChoice {
  const params = new URLSearchParams(search);
  const selected =
    params.get("renderer") ??
    (params.get("scene") === "server" ? "canvas2d" : null) ??
    saved(storage);
  const debug = params.get("engineDebug") === "1";
  const fault = debug ? FAULTS.find((f) => f === params.get("gpuFault")) : undefined;
  const atlas = ["1", "true"].includes(params.get("glyphAtlas") ?? "");
  const canvas = selected === "canvas2d";
  return {
    renderer: canvas ? "canvas2d" : "vello",
    backend: (selected && BACKENDS[selected]) || "webgpu",
    textMode: atlas ? "atlas" : "outlines",
    ...(fault ? { gpuFault: fault } : {}),
  };
}

function saved(storage?: Pick<Storage, "getItem">): string | null {
  try {
    const store = storage ?? globalThis.localStorage;
    return (
      store?.getItem(RENDERER_STORAGE_KEY) ??
      (store?.getItem(SCENE_STORAGE_KEY) === "server" ? "canvas2d" : null)
    );
  } catch {
    return null;
  }
}

/**
 * WebGPU → WebGL2 → vello_cpu; after the CPU backend only Canvas2D remains (`null`).
 * A lost WebGPU device is rebuilt once in the worker before this is consulted.
 */
export function nextBackend(failed: Backend): Backend | null {
  return failed === "webgpu" ? "webgl2" : failed === "webgl2" ? "cpu" : null;
}
