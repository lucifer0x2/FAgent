import { checkedResourceBytes } from "../../../renderer/resource-bytes";
import type { EngineModule } from "./engineApi";
import type { PageMessage, WorkerMessage } from "./protocol";
import { cachedFontBytes } from "./fontCache";
import { SceneCache } from "./sceneCache";
import { engineRuntime } from "./workerRuntime";
import { messageYield } from "./yieldTask";

const scope = globalThis as unknown as {
  onmessage: (event: MessageEvent<PageMessage>) => void;
  postMessage: (message: WorkerMessage, transfer?: Transferable[]) => void;
  requestAnimationFrame?: (callback: () => void) => number;
  navigator: { gpu?: unknown; storage?: { persist?: () => Promise<boolean> } };
};

/**
 * Persistent scene cache identity: the module's content hash (its manifest file name) and
 * the record format. A module without a hash keeps nothing on disk.
 */
let build: string | undefined;

/** The hash-named module comes from a no-store manifest written by build-engine-wasm. */
async function loadModule(): Promise<EngineModule> {
  const base = `${import.meta.env.BASE_URL}engine-wasm/`;
  const response = await fetch(`${base}manifest.json`, { cache: "no-store" });
  if (!response.ok) throw new Error(`manifest ${response.status}`);
  const manifest: { js: string; wasm: string } = await response.json();
  // Absolute URL: the dev server must serve the public file as-is, not transform it.
  const url = (name: string) => new URL(`${base}${name}`, self.location.href).href;
  const wasm = await import(/* @vite-ignore */ url(manifest.js));
  await wasm.default({ module_or_path: url(manifest.wasm) });
  const module = wasm as EngineModule;
  const hash = /-([0-9a-f]{12,64})\.wasm$/.exec(manifest.wasm)?.[1];
  const format = module.record_format?.();
  build = hash && format ? `${hash}.${format}` : undefined;
  // Best effort; browsers expose persist() to windows only, so in a worker this is a no-op
  // there (no permission prompt is ever raised from here).
  if (build) void scope.navigator.storage?.persist?.().catch(() => false);
  return module;
}

async function decodeImage(bytes: ArrayBuffer, mediaType: string) {
  const bitmap = await createImageBitmap(new Blob([bytes], { type: mediaType }), {
    premultiplyAlpha: "none",
    colorSpaceConversion: "none",
  });
  try {
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
    const context = canvas.getContext("2d", { willReadFrequently: true })!;
    context.drawImage(bitmap, 0, 0);
    const data = context.getImageData(0, 0, bitmap.width, bitmap.height).data;
    return {
      data: new Uint8Array(data.buffer),
      width: bitmap.width,
      height: bitmap.height,
    };
  } finally {
    bitmap.close();
  }
}

/** Straight RGBA8 of a small image (the WebGPU present probe reads its frames back). */
function samplePixels(image: ImageBitmap) {
  const canvas = new OffscreenCanvas(image.width, image.height);
  const context = canvas.getContext("2d", { willReadFrequently: true })!;
  context.drawImage(image, 0, 0);
  return context.getImageData(0, 0, image.width, image.height).data;
}

const checked = (manifest: { id: string; hash: string; url: string; mediaType?: string }) =>
  checkedResourceBytes({ mediaType: "", ...manifest }, 10_000);

const handle = engineRuntime({
  samplePixels,
  loadModule,
  post: (message, transfer = []) => scope.postMessage(message, transfer),
  createCanvas: () => new OffscreenCanvas(1, 1),
  fetchBytes: checked,
  fetchFont: cachedFontBytes(checked, {
    enabled: () => Boolean(build),
    storage: typeof caches === "undefined" ? undefined : caches,
    origin: self.location.origin,
  }),
  decodeImage,
  nextFrame: (callback) =>
    scope.requestAnimationFrame
      ? scope.requestAnimationFrame(callback)
      : setTimeout(callback, 16),
  yieldTask: messageYield(),
  // GPU waits (attach, probes) poll on a timer: a MessageChannel loop would spin the worker.
  pollTask: () => new Promise((resolve) => setTimeout(resolve, 0)),
  every: (callback, ms) => {
    const timer = setInterval(callback, ms);
    return () => clearInterval(timer);
  },
  hasWebGpu: () => Boolean(scope.navigator.gpu),
  now: () => performance.now(),
  cache: new SceneCache({ build: () => build }),
});

scope.onmessage = (event) => {
  void handle(event.data);
};
