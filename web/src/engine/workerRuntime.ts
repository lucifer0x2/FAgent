import { FramePacer } from "./drawPacing.ts";
import type { EngineApi, EngineModule, SurfaceApi } from "./engineApi.ts";
import { FrameLoader, type FrameLoaderDeps } from "./frameLoader.ts";
import {
  DEFAULT_OVERSCAN,
  NO_MARGIN,
  overscanCamera,
  overscanMargin,
  overscanRatio,
} from "./overscan.ts";
import { presentProbe, type PresentProbeDeps } from "./presentProbe.ts";
import type { Backend, Camera, GpuFault, PageMessage, WorkerMessage } from "./protocol.ts";

export interface RuntimeDeps
  extends Omit<FrameLoaderDeps, "engine" | "requestRender" | "post">,
    Pick<PresentProbeDeps, "samplePixels" | "pollTask"> {
  loadModule(): Promise<EngineModule>;
  post(message: WorkerMessage, transfer?: Transferable[]): void;
  /** A fresh canvas for a backend to draw into (a canvas keeps its first context). */
  createCanvas(): OffscreenCanvas;
  /** One callback before the next display frame (worker rAF, or a timer). */
  nextFrame(callback: () => void): void;
  /** Repeats `callback` every `ms` until the returned function is called. */
  every(callback: () => void, ms: number): () => void;
  /** Whether WebGPU exists in this worker (`navigator.gpu`). */
  hasWebGpu(): boolean;
  now(): number;
}

const PROBE_TIMEOUT_MS = 3_000;
const LOSS_CHECK_MS = 1_000;

const reason = (cause: unknown) =>
  typeof (cause as { message?: unknown })?.message === "string"
    ? (cause as { message: string }).message
    : String(cause);

/** A trap (Rust panic with `panic=abort`): the module can make no further calls. */
const aborted = (cause: unknown) =>
  typeof WebAssembly !== "undefined" && cause instanceof WebAssembly.RuntimeError;

/**
 * Owns the engine module in its Worker: backend attach + probes, WebGPU device-loss
 * recovery (rebuild once, then report so the page moves to WebGL2), WebGPU failure
 * detection (uncaptured GPU errors, render/present errors, a blank canvas found by the
 * present probe: reported so the page moves to WebGL2), the frame loader and one
 * coalesced render per display frame.
 *
 * Each render is handed to the page as an ImageBitmap with the layout and camera it was
 * drawn with. The image covers the page's viewport plus an overscan margin on each side
 * (`overscan.ts`: the ratio the page sends, 0 by default; clamped to the backend's max
 * texture size), so while a new image is pending the page can slide the old one during a
 * pan without exposing a blank strip. The page shows it and lines it up with its own camera
 * in one main-thread task, so the image never lags or jumps against the overlay, even while
 * a frame computation keeps this worker from drawing for most of a second.
 */
export function engineRuntime(deps: RuntimeDeps) {
  let module: EngineModule | undefined;
  let engine: EngineApi | undefined;
  const booted = (async () => {
    const start = deps.now();
    try {
      module = await deps.loadModule();
      engine = new module.Engine();
      deps.post({ type: "ready", loadMs: deps.now() - start });
    } catch (cause) {
      deps.post({ type: "fatal", error: `Engine module unavailable: ${reason(cause)}` });
    }
  })();
  let canvas: OffscreenCanvas | undefined;
  let backend: Backend | undefined;
  let rebuilt = false;
  let attaching = false;
  let dirty = false;
  /** A camera, layout or backend change is pending: drawn at the next opportunity. */
  let urgent = false;
  let scheduled = false;
  /** Committed frames' messages, shown progressively (`drawPacing.ts`). */
  const pacer = new FramePacer(deps.now, (message) => deps.post(message));
  /** `synced` of a load whose last draw failed: posted after the image that shows its frames. */
  let completion: WorkerMessage | undefined;
  let layoutSeq = 0;
  /** The page's camera (its viewport), and the overscan margin drawn around it. */
  let camera: Camera | undefined;
  let margin = NO_MARGIN;
  /** Overscan ratio per side, from the page (`overscan` message); 0 draws the viewport only. */
  let ratio = DEFAULT_OVERSCAN;
  let maxTexture = 0;
  const timings: [number, number][] = [];
  let stopWatch = () => {};
  // A loss or GPU error can arrive while nothing renders; poll so the canvas does not
  // stay blank.
  const watchLoss = () => {
    stopWatch();
    stopWatch = deps.every(() => {
      if (attaching || backend !== "webgpu" || !engine) return;
      if (engine.device_lost()) void recover();
      else checkGpuError();
    }, LOSS_CHECK_MS);
  };

  /** Leaves the current backend: the page attaches a fresh canvas for the next one. */
  function failBackend(error: string) {
    const failed = backend;
    stopWatch();
    backend = undefined;
    try {
      engine?.detach();
    } catch {
      // Detaching an aborted module throws; nothing is left to release.
    }
    if (failed) deps.post({ type: "backend-failed", backend: failed, error });
  }

  /** WebGPU validation/out-of-memory/internal errors invalidate frames: leave WebGPU. */
  function checkGpuError() {
    const error = backend === "webgpu" ? engine?.gpu_error() : undefined;
    if (!error) return false;
    dirty = true;
    failBackend(`GPU error: ${error}`);
    return true;
  }

  /**
   * Draws `camera` grown by the overscan margin. Before a backend is attached its limit is
   * unknown (fallback size); each attach applies the backend's own limit.
   */
  const applyCamera = () => {
    if (!engine || !camera) return;
    maxTexture = backend ? engine.max_texture_size() : 0;
    margin = overscanMargin(camera.width, camera.height, maxTexture, ratio);
    const c = overscanCamera(camera, margin);
    engine.set_camera(c.scale, c.tx, c.ty, c.width, c.height);
  };

  /**
   * One drawing opportunity (between two frame computations, or an animation frame):
   * committed frames are shown when the pacer says so; any other change is drawn at once.
   */
  const tick = () => {
    if (pacer.due()) {
      if (dirty) render();
      // Nothing was committed since the last image, which therefore shows these frames.
      else pacer.release();
    } else if (dirty && (urgent || !pacer.pending())) render();
  };
  const schedule = () => {
    if (scheduled || !backend) return;
    scheduled = true;
    deps.nextFrame(() => {
      scheduled = false;
      tick();
      if (dirty || pacer.pending()) schedule();
    });
  };
  const requestRender = () => {
    dirty = true;
    urgent = true;
    schedule();
  };
  /** A committed frame: drawn when the pacer releases it. */
  const progress = () => {
    dirty = true;
    schedule();
  };
  /**
   * The loader's messages: committed frames wait for their draw; `synced` comes last. A
   * reused frame is shown already, unless an older sync's message for it still waits for its
   * image (its draw failed): then the reused one waits too, so no skeleton goes first.
   */
  const loaderPost = (message: WorkerMessage) => {
    if (
      message.type === "frame" &&
      !message.error &&
      (!message.reused || pacer.holds(message.frameId))
    )
      return pacer.hold(message);
    if (message.type === "synced") {
      // The last frames must not wait for an animation frame: a worker gets none while its
      // page is hidden or not composited (seen in an embedded browser pane).
      if (dirty && !render(true)) {
        // Not drawn (no backend, a failed draw): the held frames and `synced` wait for the
        // image the rebuilt or next backend draws, so no skeleton goes before its pixels.
        completion = message;
        return;
      }
      pacer.release();
    }
    deps.post(message);
  };
  /**
   * Once a backend is attached, draws what arrived meanwhile (a layout, a camera) at once,
   * like the last frame of a sync. The page keeps its canvas hidden until that layout is
   * drawn, and a hidden or embedded page gives this worker no animation frames. With nothing
   * pending, the first image of the new surface waits for the next animation frame.
   */
  const drawAttached = () => {
    if (dirty) render();
    else requestRender();
  };
  const loader = new FrameLoader({
    ...deps,
    engine: () => engine!,
    post: loaderPost,
    requestRender: progress,
    // Frame computation keeps this worker busy for seconds, which starves its animation
    // frames. Between frames, after queued layout/camera messages have run, draw what is
    // due: a pan, zoom or layout made during the computation is drawn before the next frame
    // starts, and committed frames appear progressively, paced by `drawPacing.ts`.
    yieldTask: async () => {
      await deps.yieldTask();
      // Backend setup awaits the GPU between these steps; computing frames meanwhile
      // would hold each of its steps back by a whole frame (seconds before anything
      // shows), and nothing can be drawn until it is done.
      while (attaching) await deps.pollTask();
      tick();
    },
  });

  async function createSurface(kind: Backend, target: OffscreenCanvas) {
    const { Surface } = module!;
    if (kind === "webgpu") {
      if (!deps.hasWebGpu()) throw new Error("WebGPU is unavailable in this worker");
      return Surface.webgpu(target);
    }
    return kind === "webgl2" ? Surface.webgl2(target) : Surface.cpu(target);
  }

  async function probe() {
    engine!.start_probe();
    const deadline = deps.now() + PROBE_TIMEOUT_MS;
    for (;;) {
      const status = engine!.probe_status();
      if (status === "ok") return;
      if (status !== "pending") throw new Error(status);
      if (deps.now() > deadline) throw new Error("renderer probe timed out");
      await deps.pollTask();
    }
  }

  /**
   * Surface creation and probes on `target`; any failure leaves no surface attached.
   * WebGPU also runs the present probe (the canvas path Vello's probe does not cover).
   */
  async function bind(kind: Backend, target: OffscreenCanvas, fault?: GpuFault) {
    let surface: SurfaceApi | undefined;
    try {
      surface = await createSurface(kind, target);
      const initMs = surface.init_ms;
      engine!.attach(surface);
      surface = undefined;
      if (kind === "webgpu" && fault) engine!.simulate_gpu_fault(fault);
      await probe();
      if (kind === "webgpu") await presentProbe(engine!, target, deps);
      backend = kind;
      canvas = target;
      applyCamera();
      return initMs;
    } catch (cause) {
      surface?.free();
      try {
        engine!.detach();
      } catch {
        // An aborted module: the attach reports the original error.
      }
      backend = undefined;
      throw cause;
    }
  }

  async function attach(message: Extract<PageMessage, { type: "attach" }>) {
    await booted;
    if (!engine) return;
    attaching = true;
    stopWatch();
    let attached = false;
    try {
      engine.set_text_mode(message.textMode);
      const initMs = await bind(message.backend, deps.createCanvas(), message.fault);
      deps.post({ type: "backend", backend: message.backend, initMs });
      if (message.backend === "webgpu") watchLoss();
      attached = true;
    } catch (cause) {
      if (aborted(cause)) {
        engine = undefined;
        deps.post({ type: "fatal", error: `Engine module aborted: ${reason(cause)}` });
      } else
        deps.post({
          type: "backend-failed",
          backend: message.backend,
          error: reason(cause),
        });
    } finally {
      attaching = false;
    }
    if (attached) drawAttached();
  }

  /** Lost WebGPU device: one rebuild on the same canvas, then hand back to the page. */
  async function recover() {
    if (attaching || backend !== "webgpu" || !canvas) return;
    attaching = true;
    stopWatch();
    backend = undefined;
    let attached = false;
    try {
      if (rebuilt) throw new Error("WebGPU device lost again");
      rebuilt = true;
      const initMs = await bind("webgpu", canvas);
      deps.post({ type: "backend", backend: "webgpu", initMs, rebuilt: true });
      watchLoss();
      attached = true;
    } catch (cause) {
      engine?.detach();
      deps.post({
        type: "backend-failed",
        backend: "webgpu",
        error: `WebGPU device lost: ${reason(cause)}`,
      });
    } finally {
      attaching = false;
    }
    if (attached) drawAttached();
  }

  /**
   * `releaseAll`: the last image of a sync also shows every held frame. Returns whether an
   * image was posted.
   */
  function render(releaseAll = false): boolean {
    if (!engine || !backend || attaching) return false;
    if (backend === "webgpu" && !engine.device_lost() && checkGpuError()) return false;
    const releasing = releaseAll || completion !== undefined || pacer.due();
    dirty = false;
    urgent = false;
    try {
      const start = deps.now();
      engine.render();
      const image = canvas!.transferToImageBitmap();
      const ms = deps.now() - start;
      pacer.drew(ms);
      // The frames this image shows first, then the image, in one burst: the page shows the
      // image in the task that receives it and drops skeletons at its next publication.
      if (releasing) pacer.release();
      deps.post(
        { type: "drawn", layout: layoutSeq, ...(camera ? { camera, margin } : {}), image },
        [image],
      );
      // A load whose own last draw failed completes after the image showing its frames.
      if (completion) deps.post(completion);
      completion = undefined;
      // Recent draw times (encode + submit, worker thread) for diagnostics and benchmarks.
      timings.push([Math.round(start * 10) / 10, Math.round(ms * 10) / 10]);
      if (timings.length > 600) timings.splice(0, timings.length - 600);
      return true;
    } catch (cause) {
      // Not drawn: the rebuilt device or the next backend draws it.
      dirty = true;
      if (aborted(cause)) {
        // A Rust panic: the module is unusable, so no backend of it can draw. The page
        // moves to Canvas2D instead of keeping a blank canvas.
        stopWatch();
        backend = undefined;
        engine = undefined;
        deps.post({ type: "fatal", error: `Engine module aborted: ${reason(cause)}` });
      } else if (backend === "webgpu" && engine.device_lost()) void recover();
      else failBackend(reason(cause));
      return false;
    }
  }

  return async (message: PageMessage) => {
    switch (message.type) {
      case "attach":
        return attach(message);
      case "sync":
        await booted;
        if (!engine) return;
        // Its first committed frame is drawn at once, the next ones paced, and the last ones
        // before `synced` is posted (`loaderPost`). An older load's pending `synced` is for a
        // generation the page no longer waits for.
        completion = undefined;
        pacer.startSync();
        await loader.sync(message);
        return;
      case "layout":
        await booted;
        if (engine) loader.setLayout(message.placements);
        layoutSeq = message.seq;
        // Drawn now, not at the next animation frame: a page switch must replace the
        // previous page before a sync that follows starts computing frames.
        requestRender();
        render();
        return;
      case "camera":
        await booted;
        camera = message.camera;
        applyCamera();
        requestRender();
        return;
      case "overscan":
        await booted;
        if (overscanRatio(message.ratio) === ratio) return;
        ratio = overscanRatio(message.ratio);
        applyCamera();
        if (camera) requestRender();
        return;
      case "hit":
        await booted;
        return deps.post({
          type: "reply",
          id: message.id,
          value: engine
            ? JSON.parse(engine.hit_test(message.x, message.y, message.revision))
            : null,
        });
      case "bounds":
        await booted;
        return deps.post({
          type: "reply",
          id: message.id,
          value: engine
            ? JSON.parse(
                engine.bounds(
                  message.frameId,
                  message.revision,
                  JSON.stringify(message.ids),
                ),
              )
            : [],
        });
      case "status":
        await booted;
        return deps.post({
          type: "reply",
          id: message.id,
          value: engine
            ? {
                ...JSON.parse(engine.frame_status()),
                idle: engine.gpu_idle(),
                /** Device px drawn beyond each viewport side; `maxTexture` 0 = unknown. */
                overscan: { ratio, ...margin, maxTexture },
                /** `[startMs, drawMs]` of recent renders, worker clock. */
                renders: timings,
                now: deps.now(),
                /** Persistent scene cache counters (Task 42), when the worker has one. */
                ...(deps.cache ? { cache: deps.cache.stats() } : {}),
              }
            : null,
        });
      case "lose-device":
        engine?.lose_device();
        return;
      case "gpu-fault":
        if (backend !== "webgpu" || attaching) return;
        engine?.simulate_gpu_fault(message.fault);
        requestRender();
        return;
    }
  };
}
