import { nextBackend } from "./flag.ts";
import { DEFAULT_OVERSCAN, NO_MARGIN, overscanRatio, type Margin } from "./overscan.ts";
import type {
  Backend,
  Camera,
  FrameRequest,
  GpuFault,
  NodeBounds,
  PageMessage,
  Placement,
  WorkerMessage,
} from "./protocol.ts";
import type { Snapshot } from "../types.ts";

export const ENGINE_FALLBACK = "Vello renderer unavailable; using Canvas2D";

export interface EngineState {
  /** `failed`: the page must draw with Canvas2D for the rest of this session. */
  phase: "idle" | "booting" | "attaching" | "ready" | "failed";
  backend?: Backend;
  /** Bumped whenever the page must mount a fresh canvas (a canvas keeps its context type). */
  canvasKey: number;
  /** Backend for the next attach. */
  next: Backend;
  /** Fallback steps and the final reason, in order. */
  notes: string[];
  error?: string;
  /** Module fetch + compile + instantiate, and backend init + probe, ms (diagnostics). */
  loadMs?: number;
  initMs?: number;
  /**
   * A layout without some shown frame (page switch, removed frame) was sent: the canvas
   * stays hidden until the worker reports drawing this layout, so the previous page never
   * shows under the new page's skeletons.
   */
  hideUntilLayout?: number;
}

type FrameEvent = Extract<WorkerMessage, { type: "frame" | "synced" }>;

const LABELS: Record<Backend, string> = {
  webgpu: "WebGPU",
  webgl2: "WebGL2",
  cpu: "vello_cpu",
};
export const backendLabel = (backend?: Backend) =>
  backend ? LABELS[backend] : "";

/**
 * Page side of the engine Worker: one per tab. Owns the fallback chain (a failed backend
 * gets a fresh canvas for the next one), snapshot transfer once per revision, and
 * request/reply for hit tests, bounds and status. Scenes never come back.
 */
export class EngineClient {
  private worker?: Worker;
  private state: EngineState = { phase: "idle", canvasKey: 0, next: "webgpu", notes: [] };
  private listeners = new Set<() => void>();
  private frameListeners = new Set<(event: FrameEvent) => void>();
  private presentedListeners = new Set<() => void>();
  private presentedCamera?: Camera;
  private presentedMargin: Margin = NO_MARGIN;
  private replies = new Map<number, (value: unknown) => void>();
  private sequence = 0;
  private generation = 0;
  /** The snapshot the worker holds: project, revision and the ETag it arrived with. */
  private sent?: { projectId: string; revision: number; etag?: string };
  private lastCamera = "";
  private lastLayout = "";
  private layoutSeq = 0;
  private placed = new Set<string>();
  private textMode: "outlines" | "atlas" = "outlines";
  /** Overscan ratio from the listener (`/api/v1/viewer`); sent to each worker it starts. */
  private overscan = DEFAULT_OVERSCAN;
  /** Debug fault for the first WebGPU attach only (`?engineDebug=1&gpuFault=`). */
  private fault?: GpuFault;
  /** A canvas is attached once (StrictMode runs effects twice). */
  private attached = new WeakSet<HTMLCanvasElement>();
  /** The mounted engine canvas, showing the worker's images. */
  private output?: ImageBitmapRenderingContext;

  private createWorker: () => Worker;

  constructor(
    createWorker = () =>
      new Worker(new URL("./engine.worker.ts", import.meta.url), {
        type: "module",
      }),
  ) {
    this.createWorker = createWorker;
  }

  getState = () => this.state;

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  onFrame(listener: (event: FrameEvent) => void) {
    this.frameListeners.add(listener);
    return () => this.frameListeners.delete(listener);
  }

  /**
   * Camera of the image the engine canvas shows now (none before its first render). The
   * worker cannot redraw while it computes a frame, so the page lines this image up with
   * its own camera instead. Listeners run outside React, in the task that shows the
   * image, so a new image and its placement reach the screen in the same frame.
   */
  presented = () => this.presentedCamera;

  /** Overscan of the shown image: device px drawn beyond each side of its camera. */
  presentedOverscan = () => this.presentedMargin;

  onPresented(listener: () => void) {
    this.presentedListeners.add(listener);
    return () => {
      this.presentedListeners.delete(listener);
    };
  }

  private present(camera: Camera | undefined, margin: Margin = NO_MARGIN) {
    if (camera === this.presentedCamera && margin === this.presentedMargin) return;
    this.presentedCamera = camera;
    this.presentedMargin = margin;
    for (const listener of this.presentedListeners) listener();
  }

  private update(patch: Partial<EngineState>) {
    this.state = { ...this.state, ...patch };
    for (const listener of this.listeners) listener();
  }

  /** Starts the worker once; `backend` is the first backend of this session's chain. */
  start(backend: Backend, textMode: "outlines" | "atlas", fault?: GpuFault) {
    if (this.state.phase !== "idle") return;
    this.textMode = textMode;
    this.fault = fault;
    this.update({ phase: "booting", next: backend });
    try {
      const worker = (this.worker = this.createWorker());
      worker.onmessage = (event: MessageEvent<WorkerMessage>) =>
        this.receive(event.data);
      worker.onerror = (event) => {
        event.preventDefault?.();
        this.fail("Engine worker failed to start.");
      };
      if (this.overscan !== DEFAULT_OVERSCAN)
        this.post({ type: "overscan", ratio: this.overscan });
    } catch (cause) {
      this.fail(cause instanceof Error ? cause.message : "Engine worker failed.");
    }
  }

  /**
   * Shows the worker's images on `canvas` and starts the pending backend (once per canvas
   * element). The worker draws on its own OffscreenCanvas and posts each render as an
   * ImageBitmap: a canvas transferred to the worker would reach the screen on its own
   * schedule, out of step with the overlay and camera placement painted here.
   */
  attach(canvas: HTMLCanvasElement) {
    if (!this.worker || this.state.phase === "failed" || this.attached.has(canvas))
      return;
    this.attached.add(canvas);
    const output = canvas.getContext("bitmaprenderer");
    if (!output) return this.fail("bitmaprenderer canvas unavailable.");
    this.output = output;
    // A camera and layout are resent to the new surface, which shows nothing yet.
    this.lastCamera = this.lastLayout = "";
    this.present(undefined);
    this.update({ phase: "attaching" });
    const fault = this.state.next === "webgpu" ? this.fault : undefined;
    this.fault = undefined;
    this.post({
      type: "attach",
      backend: this.state.next,
      textMode: this.textMode,
      ...(fault ? { fault } : {}),
    });
  }

  private fail(error: string) {
    this.worker?.terminate();
    this.worker = undefined;
    for (const reply of this.replies.values()) reply(null);
    this.replies.clear();
    this.update({
      phase: "failed",
      backend: undefined,
      error,
      notes: [...this.state.notes, error],
    });
  }

  private receive(message: WorkerMessage) {
    switch (message.type) {
      case "ready":
        return this.update({ loadMs: message.loadMs });
      case "fatal":
        console.warn(`[fagent engine] ${message.error}; using Canvas2D`);
        return this.fail(message.error);
      case "backend":
        return this.update({
          phase: "ready",
          backend: message.backend,
          initMs: message.initMs,
          notes: message.rebuilt
            ? [...this.state.notes, "WebGPU device lost; rebuilt once"]
            : this.state.notes,
        });
      case "backend-failed": {
        const next = nextBackend(message.backend);
        const note = `${LABELS[message.backend]} unavailable: ${message.error}`;
        // The console line is what a user can copy from a browser we cannot automate.
        console.warn(`[fagent engine] ${note}${next ? `; trying ${LABELS[next]}` : ""}`);
        if (!next) return this.fail(note);
        return this.update({
          phase: "attaching",
          backend: undefined,
          next,
          canvasKey: this.state.canvasKey + 1,
          notes: [...this.state.notes, `${note}; trying ${LABELS[next]}`],
        });
      }
      case "frame":
      case "synced":
        for (const listener of this.frameListeners) listener(message);
        return;
      case "reply": {
        const reply = this.replies.get(message.id);
        this.replies.delete(message.id);
        reply?.(message.value);
        return;
      }
      case "rendered":
        return;
      case "drawn":
        if (this.output) this.output.transferFromImageBitmap(message.image);
        else message.image.close();
        this.present(message.camera, message.margin);
        if (
          this.state.hideUntilLayout !== undefined &&
          message.layout >= this.state.hideUntilLayout
        )
          this.update({ hideUntilLayout: undefined });
        return;
    }
  }

  private post(message: PageMessage) {
    this.worker?.postMessage(message);
  }

  private request<T>(message: PageMessage & { id: number }, fallback: T): Promise<T> {
    if (!this.worker) return Promise.resolve(fallback);
    return new Promise((resolve) => {
      this.replies.set(message.id, (value) => resolve((value ?? fallback) as T));
      this.post(message);
    });
  }

  /**
   * Computes `frames` at the snapshot's revision; returns the generation of its events.
   * `etag`: the strong ETag the server sent with this snapshot (scene cache key). A new ETag
   * at the same revision is a new representation (`snapshotHttp.ts` accepts a 200 with new
   * bytes at an equal revision), so the snapshot crosses again with it: the worker files
   * frame digests under an ETag only together with the snapshot they were computed from.
   */
  sync(snapshot: Snapshot, frames: FrameRequest[], bypass: boolean, etag?: string | null) {
    const generation = ++this.generation;
    const tag = etag || undefined;
    const held =
      this.sent?.projectId === snapshot.projectId &&
      this.sent.revision === snapshot.revision &&
      this.sent.etag === tag &&
      !bypass;
    this.post({
      type: "sync",
      generation,
      projectId: snapshot.projectId,
      revision: snapshot.revision,
      ...(held ? {} : { snapshot }),
      frames,
      bypass,
      ...(tag ? { etag: tag } : {}),
    });
    this.sent = { projectId: snapshot.projectId, revision: snapshot.revision, etag: tag };
    return generation;
  }

  setLayout(placements: Placement[]) {
    const key = JSON.stringify(placements);
    if (key === this.lastLayout) return;
    this.lastLayout = key;
    const seq = ++this.layoutSeq;
    const ids = new Set(placements.map((placement) => placement.id));
    const dropped = [...this.placed].some((id) => !ids.has(id));
    this.placed = ids;
    this.post({ type: "layout", placements, seq });
    if (dropped && this.worker) this.update({ hideUntilLayout: seq });
  }

  setCamera(camera: Camera) {
    const key = JSON.stringify(camera);
    if (key === this.lastCamera) return;
    this.lastCamera = key;
    this.post({ type: "camera", camera });
  }

  /**
   * Fraction of the viewport the worker draws beyond each edge (0: none). Known only once
   * the listener's capabilities load, so it may arrive before or after the worker starts.
   */
  setOverscan(value: number) {
    const ratio = overscanRatio(value);
    if (ratio === this.overscan) return;
    this.overscan = ratio;
    this.post({ type: "overscan", ratio });
  }

  /** Topmost node at board point (`x`, `y`) among frames current at `revision`. */
  hitTest(x: number, y: number, revision: number) {
    return this.request<NodeBounds | null>(
      { type: "hit", id: ++this.sequence, x, y, revision },
      null,
    );
  }

  /** `frameId` "" searches every placed frame; `revision` -1 accepts stale frames. */
  bounds(frameId: string, revision: number, ids: string[]) {
    return this.request<NodeBounds[]>(
      { type: "bounds", id: ++this.sequence, frameId, revision, ids },
      [],
    );
  }

  status() {
    return this.request<unknown>({ type: "status", id: ++this.sequence }, null);
  }

  /** Test hook (`?engineDebug=1`): destroys the WebGPU device. */
  loseDevice() {
    this.post({ type: "lose-device" });
  }

  /** Test hook: a simulated WebGPU failure on the attached WebGPU surface. */
  gpuFault(fault: GpuFault) {
    this.post({ type: "gpu-fault", fault });
  }
}

export const engineClient = new EngineClient();
