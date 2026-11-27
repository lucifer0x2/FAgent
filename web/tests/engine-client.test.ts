// Page side of the engine (`web/src/engine/client.ts`): the fallback chain's canvases, the
// snapshot crossing once per representation, image placement, hidden layouts and overscan.
// Worker-side runtime tests live in engine-runtime.test.ts.
import assert from "node:assert/strict";
import test from "node:test";
import { EngineClient } from "../src/engine/client.ts";
import type { WorkerMessage } from "../src/engine/protocol.ts";

class FakeWorker {
  posted: unknown[] = [];
  onmessage?: (event: { data: WorkerMessage }) => void;
  onerror?: (event: { preventDefault(): void }) => void;
  terminated = false;
  postMessage(message: unknown) {
    this.posted.push(message);
  }
  terminate() {
    this.terminated = true;
  }
  emit(data: WorkerMessage) {
    this.onmessage?.({ data });
  }
}

/** A page canvas whose `bitmaprenderer` context records the images it shows. */
function canvas(shown: unknown[] = []) {
  return {
    getContext: (kind: string) =>
      kind === "bitmaprenderer"
        ? { transferFromImageBitmap: (image: unknown) => shown.push(image) }
        : null,
  } as unknown as HTMLCanvasElement;
}

const image = (closed: unknown[] = []) => {
  const bitmap = { close: () => closed.push(bitmap) };
  return bitmap as unknown as ImageBitmap;
};

test("client: each failure mounts a fresh canvas for the next backend, then gives up", () => {
  const worker = new FakeWorker();
  const client = new EngineClient(() => worker as unknown as Worker);
  client.start("webgpu", "outlines");
  const first = canvas();
  client.attach(first);
  client.attach(first);
  assert.equal(
    worker.posted.filter((m) => (m as { type: string }).type === "attach").length,
    1,
    "a canvas transfers once (StrictMode effects run twice)",
  );
  worker.emit({ type: "backend-failed", backend: "webgpu", error: "no adapter" });
  assert.equal(client.getState().canvasKey, 1);
  assert.equal(client.getState().next, "webgl2");
  client.attach(canvas());
  worker.emit({ type: "backend-failed", backend: "webgl2", error: "no context" });
  client.attach(canvas());
  worker.emit({ type: "backend", backend: "cpu", initMs: 1 });
  assert.equal(client.getState().phase, "ready");
  assert.equal(client.getState().backend, "cpu");
  assert.deepEqual(client.getState().notes, [
    "WebGPU unavailable: no adapter; trying WebGL2",
    "WebGL2 unavailable: no context; trying vello_cpu",
  ]);
  worker.emit({ type: "backend-failed", backend: "cpu", error: "render failed" });
  assert.equal(client.getState().phase, "failed");
  assert.ok(worker.terminated);
});

test("client: the snapshot crosses once per revision unless Retry bypasses it", () => {
  const worker = new FakeWorker();
  const client = new EngineClient(() => worker as unknown as Worker);
  client.start("cpu", "outlines");
  const snapshot = { projectId: "p", revision: 3 } as never;
  client.sync(snapshot, [], false);
  client.sync(snapshot, [], false);
  client.sync(snapshot, [], true);
  client.sync({ projectId: "p", revision: 4 } as never, [], false);
  const sent = worker.posted
    .filter((m) => (m as { type: string }).type === "sync")
    .map((m) => Boolean((m as { snapshot?: unknown }).snapshot));
  assert.deepEqual(sent, [true, false, true, true]);
  // Same revision, new ETag (a 200 with new bytes at an equal revision): the new snapshot
  // crosses with its ETag, so the worker never files digests of the old one under it.
  worker.posted.length = 0;
  const tagged = { projectId: "p", revision: 5, nodes: [{ id: "old" }] };
  const rebuilt = { projectId: "p", revision: 5, nodes: [{ id: "new" }] };
  client.sync(tagged as never, [], false, '"a"');
  client.sync(tagged as never, [], false, '"a"');
  client.sync(rebuilt as never, [], false, '"b"');
  client.sync(rebuilt as never, [], false, null);
  const syncs = worker.posted.filter((m) => (m as { type: string }).type === "sync") as {
    snapshot?: unknown;
    etag?: string;
  }[];
  assert.deepEqual(
    syncs.map((m) => ({ snapshot: m.snapshot, etag: m.etag })),
    [
      { snapshot: tagged, etag: '"a"' },
      { snapshot: undefined, etag: '"a"' },
      { snapshot: rebuilt, etag: '"b"' },
      { snapshot: rebuilt, etag: undefined },
    ],
  );
  client.setCamera({ scale: 1, tx: 0, ty: 0, width: 1, height: 1 });
  client.setCamera({ scale: 1, tx: 0, ty: 0, width: 1, height: 1 });
  assert.equal(
    worker.posted.filter((m) => (m as { type: string }).type === "camera").length,
    1,
  );
});

test("client: each image is shown and placed in the task that receives it", () => {
  const worker = new FakeWorker();
  const client = new EngineClient(() => worker as unknown as Worker);
  client.start("webgpu", "outlines");
  const shown: unknown[] = [];
  client.attach(canvas(shown));
  assert.deepEqual(worker.posted.at(-1), {
    type: "attach",
    backend: "webgpu",
    textMode: "outlines",
  });
  const placed: unknown[] = [];
  client.onPresented(() => placed.push([shown.length, client.presented()]));
  const camera = { scale: 1, tx: 2, ty: 3, width: 10, height: 10 };
  const first = image();
  worker.emit({ type: "drawn", layout: 1, camera, image: first });
  assert.deepEqual(shown, [first]);
  assert.deepEqual(placed, [[1, camera]], "placement sees the new image's camera at once");
  client.attach(canvas()); // a fallback canvas shows nothing yet
  assert.equal(client.presented(), undefined);
});

test("client: without a canvas, a posted image is released", () => {
  const worker = new FakeWorker();
  const client = new EngineClient(() => worker as unknown as Worker);
  client.start("webgpu", "outlines");
  const closed: unknown[] = [];
  const bitmap = image(closed);
  worker.emit({ type: "drawn", layout: 1, image: bitmap });
  assert.deepEqual(closed, [bitmap]);
});

test("client: a layout that drops shown frames hides the canvas until the worker draws it", () => {
  const worker = new FakeWorker();
  const client = new EngineClient(() => worker as unknown as Worker);
  client.start("webgpu", "outlines");
  client.attach(canvas());
  const place = (...ids: string[]) =>
    ids.map((id, i) => ({ id, x: i * 10, y: 0, width: 5, height: 5 }));
  client.setLayout(place("a", "b"));
  assert.equal(client.getState().hideUntilLayout, undefined, "nothing was shown yet");
  client.setLayout(place("a", "b", "c"));
  assert.equal(client.getState().hideUntilLayout, undefined, "only frames added");
  client.setLayout(place("x", "y")); // page switch
  const seq = (worker.posted.at(-1) as { seq: number }).seq;
  assert.equal(client.getState().hideUntilLayout, seq);
  worker.emit({ type: "drawn", layout: seq - 1, image: image() });
  assert.equal(client.getState().hideUntilLayout, seq, "an older layout keeps it hidden");
  worker.emit({ type: "drawn", layout: seq, image: image() });
  assert.equal(client.getState().hideUntilLayout, undefined);
});

test("client: the image's overscan margin reaches the placement with its camera", () => {
  const worker = new FakeWorker();
  const client = new EngineClient(() => worker as unknown as Worker);
  client.start("webgpu", "outlines");
  client.attach(canvas());
  assert.deepEqual(client.presentedOverscan(), { x: 0, y: 0 });
  const camera = { scale: 1, tx: 2, ty: 3, width: 10, height: 10 };
  const seen: unknown[] = [];
  client.onPresented(() => seen.push([client.presented(), client.presentedOverscan()]));
  worker.emit({ type: "drawn", layout: 1, camera, margin: { x: 1, y: 1 }, image: image() });
  assert.deepEqual(seen, [[camera, { x: 1, y: 1 }]]);
});

test("client: the listener's overscan ratio reaches the worker, before or after it starts", () => {
  const overscans = (worker: FakeWorker) =>
    worker.posted.filter((m) => (m as { type: string }).type === "overscan");
  // Default 0: nothing is sent, the worker keeps its own default (no margin).
  const plain = new FakeWorker();
  const client = new EngineClient(() => plain as unknown as Worker);
  client.start("webgpu", "outlines");
  client.setOverscan(0);
  assert.deepEqual(overscans(plain), []);
  client.setOverscan(0.1);
  client.setOverscan(0.1);
  client.setOverscan(9);
  assert.deepEqual(overscans(plain), [
    { type: "overscan", ratio: 0.1 },
    { type: "overscan", ratio: 0 },
  ]);
  // Known before the worker starts: sent with the start.
  const early = new FakeWorker();
  const before = new EngineClient(() => early as unknown as Worker);
  before.setOverscan(0.2);
  before.start("webgpu", "outlines");
  assert.deepEqual(overscans(early), [{ type: "overscan", ratio: 0.2 }]);
});
