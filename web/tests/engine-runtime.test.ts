import assert from "node:assert/strict";
import test from "node:test";
import { nextBackend, rendererChoice } from "../src/engine/flag.ts";
import type { EngineModule } from "../src/engine/engineApi.ts";
import type { WorkerMessage } from "../src/engine/protocol.ts";
import { presentMismatch } from "../src/engine/presentProbe.ts";
import { stalePlacement } from "../src/engine/staleCamera.ts";
import {
  DEFAULT_OVERSCAN,
  FALLBACK_MAX_TEXTURE,
  overscanMargin,
  overscanRatio,
} from "../src/engine/overscan.ts";
import { engineRuntime } from "../src/engine/workerRuntime.ts";
import {
  PROGRESS_DRAW_MAX_MS,
  PROGRESS_DRAW_MIN_MS,
  progressGap,
} from "../src/engine/drawPacing.ts";

const storage = (value: string | null) => ({ getItem: () => value });

test("Vello (WebGPU) is the default; canvas2d, server scenes, webgl2 and cpu override it", () => {
  const keys = (values: Record<string, string>) => ({ getItem: (key: string) => values[key] ?? null });
  assert.deepEqual(rendererChoice("", storage(null)), {
    renderer: "vello",
    backend: "webgpu",
    textMode: "outlines",
  });
  assert.equal(rendererChoice("?renderer=vello", storage(null)).backend, "webgpu");
  assert.equal(rendererChoice("?renderer=webgl2", storage(null)).backend, "webgl2");
  assert.equal(rendererChoice("?renderer=cpu", storage(null)).backend, "cpu");
  assert.equal(rendererChoice("?renderer=canvas2d", storage(null)).renderer, "canvas2d");
  assert.equal(rendererChoice("", storage("canvas2d")).renderer, "canvas2d");
  assert.equal(rendererChoice("?renderer=vello", storage("canvas2d")).renderer, "vello");
  assert.equal(rendererChoice("?renderer=vello&glyphAtlas=1", storage(null)).textMode, "atlas");
  assert.equal(rendererChoice("?renderer=bogus", storage(null)).renderer, "vello");
  // Server scenes are a Canvas2D-only path.
  assert.equal(rendererChoice("?scene=server", storage(null)).renderer, "canvas2d");
  assert.equal(rendererChoice("?scene=server&renderer=vello", storage(null)).renderer, "vello");
  assert.equal(rendererChoice("?scene=server", keys({ "fagent.renderer": "vello" })).renderer, "canvas2d");
  assert.equal(rendererChoice("", keys({ "fagent.scene": "server" })).renderer, "canvas2d");
  assert.equal(
    rendererChoice("", keys({ "fagent.scene": "server", "fagent.renderer": "vello" })).renderer,
    "vello",
  );
  assert.equal(rendererChoice("?scene=wasm", storage(null)).renderer, "vello");
  assert.equal(rendererChoice("?engineDebug=1&gpuFault=blank", storage(null)).gpuFault, "blank");
  assert.equal(rendererChoice("?gpuFault=blank", storage(null)).gpuFault, undefined);
  assert.equal(rendererChoice("?engineDebug=1&gpuFault=bogus", storage(null)).gpuFault, undefined);
  assert.equal(
    rendererChoice("", { getItem: () => { throw new Error("blocked"); } }).renderer,
    "vello",
  );
  assert.deepEqual(
    ["webgpu", "webgl2", "cpu"].map((b) => nextBackend(b as "webgpu")),
    ["webgl2", "cpu", null],
  );
});

/** Colour the fake WebGPU canvas shows (null: blank), read by the present probe. */
let presented: number | null = null;

/**
 * A fake engine module: `fail` lists backends whose surface creation throws; WebGPU
 * surface creation waits for `gate` when given.
 */
function fakeModule(
  options: {
    fail?: string[];
    probe?: Record<string, string>;
    gate?: Promise<void>;
    /** The WebGPU canvas shows nothing (present probe). */
    blank?: boolean;
    /** `max_texture_size` of the attached backend (0: unknown). */
    maxTexture?: number;
    /** Runs at each `compute` (a test clock advancing by one frame's computation). */
    onCompute?: () => void;
    /** Whether `reuse` keeps the shown frame (default: nothing is reused). */
    reuse?: (frameId: string) => boolean;
  } = {},
) {
  const calls: string[] = [];
  /** Every `set_camera` call: the camera the surface draws with. */
  const cameras: number[][] = [];
  let lost = false;
  let gpuError: string | undefined;
  let throwNext = false;
  const surface = (kind: string) => {
    if (options.fail?.includes(kind)) throw new Error(`${kind} unavailable`);
    calls.push(`surface:${kind}`);
    return { kind: () => kind, init_ms: 1, free: () => calls.push(`free:${kind}`) };
  };
  let attached = "";
  const engine = {
    attach: (s: { kind(): string }) => {
      attached = s.kind();
      lost = false;
      gpuError = undefined;
      calls.push(`attach:${attached}`);
    },
    detach: () => calls.push("detach"),
    backend: () => attached,
    start_probe: () => undefined,
    probe_status: () => options.probe?.[attached] ?? "ok",
    device_lost: () => lost,
    lose_device: () => (lost = true),
    gpu_error: () => gpuError,
    present_probe: (_size: number, rgb: number) => {
      calls.push(`present-probe:${attached}`);
      if (throwNext) {
        throwNext = false;
        throw new Error("render: simulated WebGPU render failure");
      }
      presented = options.blank ? null : rgb;
    },
    end_present_probe: () => undefined,
    simulate_gpu_fault: (fault: string) => {
      if (fault === "error") gpuError = "Validation Error: simulated";
      if (fault === "throw") throwNext = true;
    },
    set_text_mode: () => undefined,
    set_camera: (...args: number[]) => cameras.push(args),
    max_texture_size: () => (attached ? (options.maxTexture ?? 8192) : 0),
    render: () => {
      if (lost) throw new Error("WebGPU device lost");
      if (throwNext) {
        throwNext = false;
        throw new Error("render: simulated WebGPU render failure");
      }
      calls.push(`render:${attached}`);
      return "{}";
    },
    gpu_idle: () => true,
    frame_status: () => "{}",
    // Frame loading (sync): every frame computes and commits; nothing is reused.
    load_snapshot: () => calls.push("snapshot"),
    frame_keys: () => "[]",
    reuse: (frameId: string) => options.reuse?.(frameId) ?? false,
    required_fonts: () => "[]",
    compute: (_revision: number, id: string) => {
      calls.push(`compute:${id}`);
      options.onCompute?.();
      return JSON.stringify({ assets: [], width: 10, height: 10, diagnostics: [] });
    },
    commit: (id: string) => calls.push(`commit:${id}`),
    discard: () => undefined,
    set_layout: () => calls.push("layout"),
  };
  const module = {
    Engine: function Engine() {
      return engine;
    },
    Surface: {
      webgpu: async () => {
        await options.gate;
        return surface("webgpu");
      },
      webgl2: () => surface("webgl2"),
      cpu: () => surface("cpu"),
    },
  } as unknown as EngineModule;
  return { module, calls, engine, cameras };
}

function runtime(
  module: EngineModule | Error,
  hasWebGpu = true,
  yieldTask = () => Promise.resolve(),
  now = () => 0,
  pollTask = yieldTask,
) {
  const posted: WorkerMessage[] = [];
  /** Images posted with `drawn`, in the transfer list of their message. */
  const transferred: unknown[] = [];
  const frames: (() => void)[] = [];
  let images = 0;
  const handle = engineRuntime({
    samplePixels: () => {
      const px = new Uint8ClampedArray(16 * 16 * 4);
      if (presented !== null)
        for (let i = 0; i < px.length; i += 4)
          px.set([(presented >> 16) & 255, (presented >> 8) & 255, presented & 255, 255], i);
      return px;
    },
    loadModule: async () => {
      if (module instanceof Error) throw module;
      return module;
    },
    post: (message, transfer = []) => {
      posted.push(message);
      transferred.push(...transfer);
    },
    createCanvas: () =>
      ({
        transferToImageBitmap: () => ({ id: ++images }),
      }) as unknown as OffscreenCanvas,
    nextFrame: (callback) => frames.push(callback),
    hasWebGpu: () => hasWebGpu,
    every: () => () => {},
    now,
    yieldTask,
    pollTask,
    fetchBytes: async () => new ArrayBuffer(0),
    decodeImage: async () => ({ data: new Uint8Array(4), width: 1, height: 1 }),
  });
  const flush = () => frames.splice(0).forEach((run) => run());
  const attach = (backend: "webgpu" | "webgl2" | "cpu") =>
    handle({ type: "attach", backend, textMode: "outlines" });
  return { handle, posted, transferred, flush, attach };
}

const kinds = (posted: WorkerMessage[]) =>
  posted.filter((m) => m.type !== "drawn").map((m) =>
    m.type === "backend" || m.type === "backend-failed"
      ? `${m.type}:${m.backend}${"rebuilt" in m && m.rebuilt ? ":rebuilt" : ""}`
      : m.type,
  );

test("worker reports each failed backend; the page walks WebGPU → WebGL2 → vello_cpu", async () => {
  const { module, calls } = fakeModule({ fail: ["webgl2"] });
  const { posted, flush, attach } = runtime(module, false);
  await attach("webgpu");
  await attach("webgl2");
  await attach("cpu");
  flush();
  assert.deepEqual(kinds(posted), [
    "ready",
    "backend-failed:webgpu",
    "backend-failed:webgl2",
    "backend:cpu",
  ]);
  assert.match(
    (posted[1] as { error: string }).error,
    /WebGPU is unavailable in this worker/,
  );
  assert.ok(calls.includes("render:cpu"));
});

test("a renderer probe mismatch rejects the backend before anything is drawn", async () => {
  const { module, calls } = fakeModule({ probe: { webgpu: "failed: renderer probe output does not match" } });
  const { posted, flush, attach } = runtime(module);
  await attach("webgpu");
  flush();
  assert.deepEqual(kinds(posted), ["ready", "backend-failed:webgpu"]);
  assert.ok(calls.includes("detach"));
  assert.ok(!calls.some((call) => call.startsWith("render")));
});

test("GPU waits poll on a timer task, never on the frame yield (which would spin the worker)", async () => {
  let clock = 0;
  let yields = 0;
  let polls = 0;
  const { module } = fakeModule({ probe: { webgpu: "pending" } });
  const { posted, attach } = runtime(
    module,
    true,
    async () => void (yields++, (clock += 1_000)),
    () => clock,
    async () => void (polls++, (clock += 1_000)),
  );
  await attach("webgpu");
  assert.ok(polls >= 3, "the pending probe is polled until its deadline");
  assert.equal(yields, 0);
  const failed = posted.find((m) => m.type === "backend-failed");
  assert.match(failed && "error" in failed ? failed.error : "", /probe timed out/);
});

test("present probe: a WebGPU canvas that stays blank is rejected before anything is drawn", async () => {
  const { module, calls } = fakeModule({ blank: true });
  const { posted, flush, attach } = runtime(module);
  await attach("webgpu");
  flush();
  assert.deepEqual(kinds(posted), ["ready", "backend-failed:webgpu"]);
  assert.match((posted[1] as { error: string }).error, /present probe: canvas stayed blank/);
  assert.ok(!calls.some((call) => call.startsWith("render")));
  await attach("webgl2");
  flush();
  assert.ok(!calls.includes("present-probe:webgl2"), "only WebGPU runs the present probe");
  assert.ok(calls.includes("render:webgl2"));
});

test("present probe: a working WebGPU canvas passes with both colours", async () => {
  const { module, calls } = fakeModule();
  const { posted, flush, attach } = runtime(module);
  await attach("webgpu");
  flush();
  assert.deepEqual(kinds(posted), ["ready", "backend:webgpu"]);
  assert.equal(calls.filter((c) => c === "present-probe:webgpu").length, 2);
});

test("presentMismatch: blank, wrong channel order and the expected colour", () => {
  const fill = (rgba: number[]) => {
    const px = new Uint8ClampedArray(16 * 16 * 4);
    for (let i = 0; i < px.length; i += 4) px.set(rgba, i);
    return px;
  };
  assert.match(presentMismatch(fill([0, 0, 0, 0]), 0x2080e0)!, /stayed blank/);
  assert.match(presentMismatch(fill([0xe0, 0x80, 0x20, 255]), 0x2080e0)!, /#e08020 instead of #2080e0/);
  assert.equal(presentMismatch(fill([0x22, 0x7e, 0xe0, 255]), 0x2080e0), undefined);
  assert.match(presentMismatch(new Uint8ClampedArray(0), 0x2080e0)!, /no pixels/);
});

test("an uncaptured WebGPU error or a failing render hands over to WebGL2", async () => {
  for (const fault of ["error", "throw"] as const) {
    const { module } = fakeModule();
    const { posted, flush, attach, handle } = runtime(module);
    await attach("webgpu");
    flush();
    await handle({ type: "gpu-fault", fault });
    flush();
    assert.deepEqual(kinds(posted), ["ready", "backend:webgpu", "backend-failed:webgpu"], fault);
    assert.match(
      (posted.at(-1) as { error: string }).error,
      fault === "error" ? /^GPU error: Validation Error/ : /simulated WebGPU render failure/,
    );
  }
});

test("a fault requested with the attach applies to the WebGPU surface before its probes", async () => {
  const { module } = fakeModule();
  const { posted, flush, handle } = runtime(module);
  await handle({ type: "attach", backend: "webgpu", textMode: "outlines", fault: "throw" });
  flush();
  // The present probe is the first present: it fails, so WebGPU never attaches.
  assert.deepEqual(kinds(posted), ["ready", "backend-failed:webgpu"]);
  assert.match((posted[1] as { error: string }).error, /simulated WebGPU render failure/);
});

test("a lost WebGPU device is rebuilt once; a second loss hands over to WebGL2", async () => {
  const { module, engine, calls } = fakeModule();
  const { posted, flush, attach, handle } = runtime(module);
  await attach("webgpu");
  flush();
  engine.lose_device();
  await handle({ type: "camera", camera: { scale: 1, tx: 0, ty: 0, width: 10, height: 10 } });
  flush();
  await new Promise((resolve) => setTimeout(resolve, 0));
  flush();
  engine.lose_device();
  await handle({ type: "camera", camera: { scale: 2, tx: 0, ty: 0, width: 10, height: 10 } });
  flush();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(kinds(posted), [
    "ready",
    "backend:webgpu",
    "backend:webgpu:rebuilt",
    "backend-failed:webgpu",
  ]);
  assert.match(
    (posted.at(-1) as { error: string }).error,
    /device lost.*again/i,
  );
  assert.equal(calls.filter((c) => c === "surface:webgpu").length, 2);
});

test("a module that cannot load is fatal: the page draws with Canvas2D", async () => {
  const { posted } = runtime(new Error("404 manifest"));
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(kinds(posted), ["fatal"]);
  assert.match((posted[0] as { error: string }).error, /404 manifest/);
});

const SNAPSHOT = {
  projectId: "p",
  slug: "p",
  name: "P",
  revision: 1,
  schemaVersion: 1,
  nodes: [],
  tokens: [],
  styles: [],
  assets: [],
  fonts: [],
  retention: { oldestCommitRevision: 0, idempotencyPolicy: "bounded" },
  truncated: false,
} as never;

const sync = (frames: string[]) =>
  ({
    type: "sync",
    generation: 1,
    projectId: "p",
    revision: 1,
    snapshot: SNAPSHOT,
    frames: frames.map((frameId) => ({ frameId, width: 10, height: 10 })),
    bypass: false,
  }) as const;

test("a busy worker draws the first frame at once, paces the next ones and always draws the last, without animation frames", async () => {
  // Each frame computation takes 40 ms of worker time; draws take none, so the progress
  // gap is its minimum (`drawPacing.ts`).
  let clock = 0;
  const { module, calls } = fakeModule({ onCompute: () => (clock += 40) });
  const { handle, attach, posted, transferred, flush } = runtime(
    module,
    true,
    () => Promise.resolve(),
    () => clock,
  );
  assert.equal(progressGap(0), PROGRESS_DRAW_MIN_MS);
  assert.equal(PROGRESS_DRAW_MIN_MS, 100);
  await attach("webgpu"); // animation frames never run below (`flush` is not called)
  calls.length = 0;
  const camera = { scale: 0.5, tx: 3, ty: 4, width: 10, height: 10 };
  await handle({ type: "camera", camera });
  await handle({ type: "layout", placements: [], seq: 7 });
  await handle(sync(["a", "b", "c", "d", "e", "f"]));
  assert.deepEqual(calls, [
    "layout",
    "render:webgpu", // the new (empty) layout replaces the previous page at once (t=0)
    "snapshot",
    "compute:a",
    "commit:a",
    "render:webgpu", // the first frame is shown before the next is computed (t=40)
    "compute:b",
    "commit:b",
    "compute:c",
    "commit:c",
    "compute:d",
    "commit:d",
    "render:webgpu", // b–d together, once the gap since the last draw has passed (t=160)
    "compute:e",
    "commit:e",
    "compute:f",
    "commit:f",
    "render:webgpu", // the last frames never wait for the gap or an animation frame (t=240)
  ]);
  // Each committed frame's message comes in the burst of the image that shows it, so the
  // page never drops a skeleton before the frame's pixels; `synced` comes last.
  assert.deepEqual(
    posted
      .filter((m) => m.type === "drawn" || m.type === "frame" || m.type === "synced")
      .map((m) => (m.type === "frame" ? m.frameId : m.type)),
    ["drawn", "a", "drawn", "b", "c", "d", "drawn", "e", "f", "drawn", "synced"],
  );
  const drawn = posted.filter((m) => m.type === "drawn");
  for (const message of drawn) {
    assert.equal(message.layout, 7);
    assert.deepEqual(message.camera, camera, "with the camera it was drawn with");
  }
  assert.deepEqual(
    transferred,
    drawn.map((m) => m.image),
    "images move to the page, they are not copied",
  );
  flush();
  assert.equal(posted.filter((m) => m.type === "drawn").length, 4, "nothing left to draw");
});

test("a camera or layout change during a load is drawn before the next frame, unpaced", async () => {
  let clock = 0;
  let step = async () => {};
  const { module, calls } = fakeModule({ onCompute: () => (clock += 10) });
  const { handle, attach, posted } = runtime(
    module,
    true,
    async () => {
      await Promise.resolve();
      await step();
    },
    () => clock,
  );
  await attach("webgpu");
  await handle({ type: "layout", placements: [], seq: 1 });
  calls.length = 0;
  let sent = false;
  // Between frames b and c (well inside the progress gap) the page pans.
  step = async () => {
    if (sent || !calls.includes("commit:b")) return;
    sent = true;
    await handle({
      type: "camera",
      camera: { scale: 1, tx: 1, ty: 0, width: 10, height: 10 },
    });
  };
  await handle(sync(["a", "b", "c"]));
  assert.deepEqual(calls, [
    "snapshot",
    "compute:a",
    "commit:a",
    "render:webgpu", // first frame
    "compute:b",
    "commit:b",
    "render:webgpu", // the pan (with frame b), not held back by the gap
    "compute:c",
    "commit:c",
    "render:webgpu", // last frame
  ]);
  // The pan's image shows frame b, but b's message waits for the paced release: releasing
  // it would change what the page publishes (hug heights, then a new layout to draw), one
  // draw per frame again.
  assert.deepEqual(
    posted
      .filter((m) => m.type === "drawn" || m.type === "frame" || m.type === "synced")
      .map((m) => (m.type === "frame" ? m.frameId : m.type))
      .slice(-7),
    ["a", "drawn", "drawn", "b", "c", "drawn", "synced"],
  );
});

test("a last draw that fails holds its frames and `synced` until the next backend draws them", async () => {
  let clock = 0;
  const { module, calls, engine } = fakeModule({
    onCompute: () => {
      clock += 10;
      // The draw after the last frame fails (a WebGPU render error): WebGPU is left.
      if (calls.includes("compute:c")) engine.simulate_gpu_fault("throw");
    },
  });
  const { handle, attach, posted } = runtime(module, true, () => Promise.resolve(), () => clock);
  await attach("webgpu");
  await handle({ type: "layout", placements: [], seq: 1 });
  const shown = () =>
    posted
      .filter((m) => ["drawn", "frame", "synced", "backend", "backend-failed"].includes(m.type))
      .map((m) =>
        m.type === "frame"
          ? m.frameId
          : m.type === "backend" || m.type === "backend-failed"
            ? `${m.type}:${m.backend}`
            : m.type,
      );
  await handle(sync(["a", "b", "c"]));
  // b and c are committed but in no delivered image: the page keeps their skeletons.
  assert.deepEqual(shown(), [
    "backend:webgpu",
    "drawn", // the layout
    "a",
    "drawn", // first frame
    "backend-failed:webgpu",
  ]);
  await attach("webgl2");
  assert.deepEqual(shown().slice(-5), ["backend:webgl2", "b", "c", "drawn", "synced"]);
  assert.deepEqual(calls.filter((call) => call.startsWith("render")), [
    "render:webgpu",
    "render:webgpu",
    "render:webgl2",
  ]);
});

test("a reused frame whose earlier message still waits for an image waits with it", async () => {
  let clock = 0;
  let reusing = false;
  const { module, calls, engine } = fakeModule({
    onCompute: () => {
      clock += 10;
      if (calls.includes("compute:c")) engine.simulate_gpu_fault("throw");
    },
    reuse: () => reusing,
  });
  const { handle, attach, posted } = runtime(module, true, () => Promise.resolve(), () => clock);
  await attach("webgpu");
  await handle({ type: "layout", placements: [], seq: 1 });
  await handle(sync(["a", "b", "c"]));
  // b and c wait for an image (the last draw failed). A newer sync reuses all three frames.
  reusing = true;
  const mark = posted.length;
  await handle({ ...sync(["a", "b", "c"]), generation: 2, snapshot: undefined });
  const seen = () =>
    posted
      .slice(mark)
      .filter((m) => ["drawn", "frame", "synced", "backend"].includes(m.type))
      .map((m) =>
        m.type === "frame"
          ? `${m.frameId}${m.generation}${m.reused ? "r" : ""}`
          : m.type === "synced"
            ? `synced${m.generation}`
            : m.type,
      );
  assert.deepEqual(seen(), ["a2r"], "only a, drawn before the failure, is reported");
  await attach("webgl2");
  assert.deepEqual(seen(), ["a2r", "backend", "b1", "c1", "b2r", "c2r", "drawn", "synced2"]);
});

test("progress gap: grows with the draw time between its bounds", () => {
  assert.equal(progressGap(2), PROGRESS_DRAW_MIN_MS);
  assert.equal(progressGap(16), 160);
  assert.equal(progressGap(80), PROGRESS_DRAW_MAX_MS);
});

test("frames wait for the backend: nothing computes while WebGPU is still being set up", async () => {
  let open = () => {};
  const gate = new Promise<void>((resolve) => (open = resolve));
  const { module, calls } = fakeModule({ gate });
  const { handle, attach, flush } = runtime(
    module,
    true,
    () => new Promise((resolve) => setTimeout(resolve, 0)),
  );
  const attached = attach("webgpu");
  const synced = handle(sync(["a"]));
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.ok(!calls.includes("compute:a"), "the first frame waits for the surface");
  open();
  await Promise.all([attached, synced]);
  flush();
  assert.deepEqual(calls.filter((call) => /^(surface|compute|render)/.test(call)), [
    "surface:webgpu",
    "render:webgpu",
    "compute:a",
    "render:webgpu",
  ]);
});

test("a layout sent while a backend attaches is drawn when it attaches, without animation frames", async () => {
  let open = () => {};
  const gate = new Promise<void>((resolve) => (open = resolve));
  const { module, calls, engine } = fakeModule({ gate });
  const { handle, attach, posted } = runtime(module);
  // Animation frames never run below (`flush` is not called): a hidden or embedded page.
  const attached = attach("webgpu");
  await new Promise((resolve) => setTimeout(resolve, 0));
  await handle({ type: "layout", placements: [], seq: 4 });
  assert.ok(!calls.some((call) => call.startsWith("render")), "nothing draws mid-attach");
  open();
  await attached;
  const drawn = posted.filter((m) => m.type === "drawn");
  assert.deepEqual(
    drawn.map((m) => m.layout),
    [4],
    "the pending layout is drawn once the surface is ready, so the page stops hiding it",
  );
  // A rebuilt WebGPU device draws the current layout at once too.
  engine.lose_device();
  await handle({ type: "layout", placements: [], seq: 5 });
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(kinds(posted), ["ready", "backend:webgpu", "backend:webgpu:rebuilt"]);
  assert.deepEqual(
    posted.filter((m) => m.type === "drawn").map((m) => m.layout),
    [4, 5],
  );
});

test("a stale image is moved and scaled onto the current camera", () => {
  const drawn = { scale: 2, tx: 100, ty: 40, width: 800, height: 600 };
  assert.deepEqual(stalePlacement(drawn, drawn, 2), { transform: "", width: "", height: "" });
  assert.deepEqual(stalePlacement(undefined, drawn, 2), {
    transform: "",
    width: "",
    height: "",
  });
  const current = { scale: 4, tx: -60, ty: 10, width: 800, height: 600 };
  const placed = stalePlacement(drawn, current, 2);
  const [x, y, k] = placed.transform
    .match(/translate\((-?[\d.]+)px, (-?[\d.]+)px\) scale\(([\d.]+)\)/)!
    .slice(1)
    .map(Number);
  // Board point (30, 25): device px in each camera, then CSS px through the transform.
  const at = (camera: typeof drawn, p: number, axis: "tx" | "ty") =>
    p * camera.scale + camera[axis];
  assert.equal(at(drawn, 30, "tx") / 2 * k + x, at(current, 30, "tx") / 2);
  assert.equal(at(drawn, 25, "ty") / 2 * k + y, at(current, 25, "ty") / 2);
  assert.deepEqual(
    [placed.width, placed.height],
    ["", ""],
    "same canvas size: the stylesheet size stays",
  );
  const resized = stalePlacement(drawn, { ...drawn, width: 900 }, 2);
  assert.deepEqual(resized, { transform: "", width: "400px", height: "300px" });
});

test("overscan is off by default: no margin, and only ratios in [0, 0.5] are accepted", () => {
  assert.equal(DEFAULT_OVERSCAN, 0);
  assert.deepEqual(overscanMargin(2344, 1536, 8192), { x: 0, y: 0 });
  assert.deepEqual(overscanMargin(2344, 1536, 8192, 0), { x: 0, y: 0 });
  assert.deepEqual(overscanMargin(938, 868, 0), { x: 0, y: 0 });
  for (const value of [0, 0.1, 0.125, 0.5]) assert.equal(overscanRatio(value), value);
  for (const value of [undefined, null, "0.1", -0.1, 0.51, NaN, Infinity])
    assert.equal(overscanRatio(value), 0, String(value));
  // No margin places a stale image exactly as before overscan existed.
  const drawn = { scale: 2, tx: 100, ty: 40, width: 800, height: 600 };
  for (const current of [drawn, { ...drawn, tx: 170 }, { ...drawn, scale: 3, tx: -60 }])
    assert.deepEqual(
      stalePlacement(drawn, current, 2, overscanMargin(800, 600, 8192)),
      stalePlacement(drawn, current, 2),
    );
  assert.deepEqual(stalePlacement(drawn, drawn, 2, overscanMargin(800, 600, 8192)), {
    transform: "",
    width: "",
    height: "",
  });
});

test("overscan 0.1: 10% of the viewport per side, shrunk to the max texture size, never the viewport", () => {
  assert.deepEqual(overscanMargin(2344, 1536, 8192, 0.1), { x: 234, y: 154 });
  assert.deepEqual(overscanMargin(938, 868, 0, 0.1), { x: 94, y: 87 }, "unknown limit: fallback");
  // 3800 + 2·380 > 4096: the margin shrinks to what fits.
  assert.deepEqual(overscanMargin(3800, 1000, 4096, 0.1), { x: 148, y: 100 });
  assert.deepEqual(
    overscanMargin(3800, 1000, 0, 0.1),
    overscanMargin(3800, 1000, FALLBACK_MAX_TEXTURE, 0.1),
  );
  assert.deepEqual(overscanMargin(5000, 4096, 4096, 0.1), { x: 0, y: 0 }, "a viewport over the limit");
  assert.deepEqual(overscanMargin(1000, 800, 8192, 0.25), { x: 250, y: 200 }, "the ratio given");
  for (const [w, h, max] of [[2344, 1536, 8192], [3800, 3000, 4096], [7000, 100, 8192]]) {
    const m = overscanMargin(w, h, max, 0.1);
    assert.ok(w + 2 * m.x <= max && h + 2 * m.y <= max, `${w}x${h} fits ${max}`);
  }
});

test("worker: by default the canvas is exactly the viewport; it applies the ratio it is given", async () => {
  const { module, cameras } = fakeModule({ maxTexture: 8192 });
  const { handle, attach, posted, flush } = runtime(module);
  const camera = { scale: 2, tx: 10, ty: -20, width: 1000, height: 800 };
  await handle({ type: "camera", camera });
  await attach("webgpu");
  flush();
  assert.deepEqual(cameras.at(-1), [2, 10, -20, 1000, 800], "no margin without a ratio");
  const drawn = () => posted.filter((m) => m.type === "drawn").at(-1)!;
  assert.deepEqual(drawn().margin, { x: 0, y: 0 });
  await handle({ type: "status", id: 1 });
  assert.deepEqual((posted.at(-1) as { value: { overscan: unknown } }).value.overscan, {
    ratio: 0,
    x: 0,
    y: 0,
    maxTexture: 8192,
  });
  // A ratio from the page regrows the current camera and redraws.
  await handle({ type: "overscan", ratio: 0.25 });
  assert.deepEqual(cameras.at(-1), [2, 10 + 250, -20 + 200, 1500, 1200]);
  flush();
  assert.deepEqual(drawn().margin, { x: 250, y: 200 });
  assert.deepEqual(drawn().camera, camera);
  // Out of range is ignored as 0; back to 0 is exactly the viewport again.
  await handle({ type: "overscan", ratio: 3 });
  assert.deepEqual(cameras.at(-1), [2, 10, -20, 1000, 800]);
  await handle({ type: "overscan", ratio: 0.1 });
  await handle({ type: "overscan", ratio: 0 });
  assert.deepEqual(cameras.at(-1), [2, 10, -20, 1000, 800]);
});

test("worker: the canvas is the viewport plus the overscan margin, clamped to the backend", async () => {
  const { module, cameras } = fakeModule({ maxTexture: 2048 });
  const { handle, attach, posted, flush } = runtime(module);
  const camera = { scale: 2, tx: 10, ty: -20, width: 1800, height: 1000 };
  await handle({ type: "overscan", ratio: 0.1 });
  await handle({ type: "camera", camera });
  await attach("webgpu");
  flush();
  // Before the attach the limit is unknown (fallback); the attach applies the backend's.
  assert.deepEqual(cameras.at(-1), [2, 10 + 124, -20 + 100, 1800 + 248, 1000 + 200]);
  const drawn = posted.filter((m) => m.type === "drawn").at(-1)!;
  assert.deepEqual(drawn.camera, camera, "the page camera, not the grown one");
  assert.deepEqual(drawn.margin, { x: 124, y: 100 });
  await handle({ type: "camera", camera: { ...camera, width: 1000 } });
  assert.deepEqual(cameras.at(-1), [2, 110, 80, 1200, 1200]);
  await handle({ type: "status", id: 1 });
  const reply = posted.at(-1) as { value: { overscan: unknown } };
  assert.deepEqual(reply.value.overscan, { ratio: 0.1, x: 100, y: 100, maxTexture: 2048 });
});

test("worker: vello_cpu (no reported limit) overscans within the fallback size", async () => {
  const { module, cameras } = fakeModule({ maxTexture: 0 });
  const { handle, attach, flush } = runtime(module);
  await handle({ type: "overscan", ratio: 0.1 });
  await attach("cpu");
  await handle({ type: "camera", camera: { scale: 1, tx: 0, ty: 0, width: 4000, height: 900 } });
  flush();
  assert.deepEqual(cameras.at(-1), [1, 48, 90, 4096, 1080]);
});

/** CSS rect the placed image covers: `[left, top, right, bottom]` in host px. */
function imageRect(
  placement: ReturnType<typeof stalePlacement>,
  image: { width: number; height: number },
  pixelRatio: number,
) {
  const [x, y, k] = placement.transform
    ? placement.transform
        .match(/translate\((-?[\d.e-]+)px, (-?[\d.e-]+)px\) scale\(([\d.e-]+)\)/)!
        .slice(1)
        .map(Number)
    : [0, 0, 1];
  const width = placement.width ? parseFloat(placement.width) : image.width / pixelRatio;
  const height = placement.height ? parseFloat(placement.height) : image.height / pixelRatio;
  return { x, y, k, rect: [x, y, x + width * k, y + height * k] };
}

test("a stale overscanned image stays aligned with the camera while panning and zooming", () => {
  const pixelRatio = 2;
  const drawn = { scale: 2, tx: 100, ty: 40, width: 800, height: 600 };
  const margin = overscanMargin(drawn.width, drawn.height, 8192, 0.1);
  assert.deepEqual(margin, { x: 80, y: 60 });
  const image = { width: drawn.width + 2 * margin.x, height: drawn.height + 2 * margin.y };
  // Same camera: the image sits at −margin, sized to itself; on-screen pixels are unchanged.
  const same = stalePlacement(drawn, drawn, pixelRatio, margin);
  assert.deepEqual(same, {
    transform: "translate(-40px, -30px) scale(1)",
    width: "480px",
    height: "360px",
  });
  for (const current of [
    { ...drawn, tx: drawn.tx + 70, ty: drawn.ty - 50 }, // pan within the margin
    { ...drawn, scale: 3, tx: -60, ty: 10 }, // zoom in
    { ...drawn, scale: 1.5, tx: 180, ty: 90 }, // zoom out
  ]) {
    const placed = stalePlacement(drawn, current, pixelRatio, margin);
    const { x, y, k } = imageRect(placed, image, pixelRatio);
    // Board point p: image device px p·s₀ + t₀ + m, then CSS px through the transform,
    // lands where the current camera draws it (and so where the overlay paints it).
    for (const [px, py] of [[30, 25], [-12, 400], [250, -7]]) {
      const ix = px * drawn.scale + drawn.tx + margin.x,
        iy = py * drawn.scale + drawn.ty + margin.y;
      assert.ok(Math.abs((ix / pixelRatio) * k + x - (px * current.scale + current.tx) / pixelRatio) < 1e-9);
      assert.ok(Math.abs((iy / pixelRatio) * k + y - (py * current.scale + current.ty) / pixelRatio) < 1e-9);
    }
  }
});

test("a pan within the overscan margin leaves no part of the viewport uncovered", () => {
  const pixelRatio = 1;
  const drawn = { scale: 1, tx: 0, ty: 0, width: 1000, height: 800 };
  const margin = overscanMargin(drawn.width, drawn.height, 8192, 0.1);
  const image = { width: drawn.width + 2 * margin.x, height: drawn.height + 2 * margin.y };
  const covers = (dx: number, dy: number) => {
    const current = { ...drawn, tx: dx, ty: dy };
    const { rect } = imageRect(stalePlacement(drawn, current, pixelRatio, margin), image, pixelRatio);
    return rect[0] <= 0 && rect[1] <= 0 && rect[2] >= drawn.width && rect[3] >= drawn.height;
  };
  for (const [dx, dy] of [[100, 0], [-100, 0], [0, 80], [0, -80], [60, -50], [-100, 80]])
    assert.ok(covers(dx, dy), `pan ${dx},${dy} stays covered`);
  assert.ok(!covers(101, 0) && !covers(0, -81), "beyond the margin a strip is exposed");
  // Without overscan the same pan exposes a strip (the behaviour this replaces).
  const bare = imageRect(stalePlacement(drawn, { ...drawn, tx: 40 }, 1), drawn, 1).rect;
  assert.ok(bare[0] > 0);
});
