import assert from "node:assert/strict";
import test from "node:test";
import type { EngineApi } from "../src/engine/engineApi.ts";
import { FrameLoader } from "../src/engine/frameLoader.ts";
import type { WorkerMessage } from "../src/engine/protocol.ts";
import type { Snapshot } from "../src/types.ts";

const HASH = "a".repeat(64);
const snapshot = (revision: number): Snapshot =>
  ({
    projectId: "p",
    slug: "p",
    name: "P",
    revision,
    schemaVersion: 1,
    nodes: [],
    tokens: [],
    styles: [],
    assets: [{ id: "img", contentHash: "b".repeat(64), mediaType: "image/png" }],
    fonts: [{ id: "font", family: "F", weight: 400, style: "normal", contentHash: HASH }],
    retention: { oldestCommitRevision: 0, idempotencyPolicy: "bounded" },
    truncated: false,
  }) as unknown as Snapshot;

/** Records engine calls; frames with the same digest at a later revision are reused. */
function fakeEngine(digests: Record<string, string>) {
  const calls: string[] = [];
  const fonts = new Set<string>();
  const shown = new Map<string, string>();
  const staged = new Map<string, string>();
  const engine = {
    load_snapshot: (json: string) => calls.push(`snapshot:${JSON.parse(json).revision}`),
    frame_keys: (_revision: number, frames: string) =>
      JSON.stringify(
        JSON.parse(frames).map((f: { frameId: string }) => ({
          frameId: f.frameId,
          digest: digests[f.frameId],
        })),
      ),
    reuse: (id: string, digest: string) => shown.get(id) === digest,
    required_fonts: () => JSON.stringify([{ id: "font", hash: HASH }]),
    has_font: (id: string) => fonts.has(id),
    load_font: (id: string) => {
      fonts.add(id);
      calls.push(`font:${id}`);
    },
    compute: (revision: number, id: string, _w: number, _h: number, digest: string) => {
      calls.push(`compute:${id}@${revision}`);
      staged.set(id, digest);
      return JSON.stringify({ frameId: id, revision, assets: [{ id: "img", hash: "b".repeat(64) }], diagnostics: [] });
    },
    has_image: () => false,
    add_image: (id: string) => calls.push(`image:${id}`),
    commit: (id: string) => {
      shown.set(id, staged.get(id)!);
      calls.push(`commit:${id}`);
      return true;
    },
    discard: (id: string) => calls.push(`discard:${id}`),
    // Like the engine: a layout releases every frame it leaves out.
    set_layout: (json: string) => {
      const placed = new Set(JSON.parse(json).map((p: { id: string }) => p.id));
      for (const map of [shown, staged])
        for (const id of map.keys()) if (!placed.has(id)) map.delete(id);
      calls.push("layout");
    },
  };
  return { engine: engine as unknown as EngineApi, calls };
}

function loader(
  engine: EngineApi,
  options: { fetch?: (id: string) => Promise<ArrayBuffer>; yieldTask?: () => Promise<void> } = {},
) {
  const posted: WorkerMessage[] = [];
  let renders = 0;
  const frames = new FrameLoader({
    engine: () => engine,
    post: (message) => posted.push(message),
    requestRender: () => renders++,
    yieldTask: options.yieldTask ?? (() => Promise.resolve()),
    fetchBytes: (manifest) =>
      options.fetch ? options.fetch(manifest.id) : Promise.resolve(new ArrayBuffer(4)),
    decodeImage: async () => ({ data: new Uint8Array(4), width: 1, height: 1 }),
  });
  return { frames, posted, renders: () => renders };
}

const sync = (generation: number, revision: number, withSnapshot = true, bypass = false) => ({
  type: "sync" as const,
  generation,
  projectId: "p",
  revision,
  ...(withSnapshot ? { snapshot: snapshot(revision) } : {}),
  frames: [
    { frameId: "a", width: 100, height: 100 },
    { frameId: "b", width: 100, height: 100 },
  ],
  bypass,
});

test("frames compute in order, fonts load once, unchanged frames are reused next revision", async () => {
  const { engine, calls } = fakeEngine({ a: "da", b: "db" });
  const { frames, posted, renders } = loader(engine);
  await frames.sync(sync(1, 3));
  assert.deepEqual(calls, [
    "snapshot:3",
    "font:font",
    "compute:a@3",
    "image:img",
    "commit:a",
    "compute:b@3",
    "image:img",
    "commit:b",
  ]);
  assert.equal(renders(), 2);
  assert.deepEqual(
    posted.map((m) => m.type),
    ["frame", "frame", "synced"],
  );
  calls.length = 0;
  posted.length = 0;
  await frames.sync(sync(2, 4));
  assert.deepEqual(calls, ["snapshot:4"]);
  assert.ok(posted.slice(0, 2).every((m) => m.type === "frame" && m.reused));
  calls.length = 0;
  await frames.sync(sync(3, 4, false, true));
  assert.deepEqual(calls, ["compute:a@4", "image:img", "commit:a", "compute:b@4", "image:img", "commit:b"]);
});

test("a frame that fetched a font yields before it computes, so a camera sent meanwhile is drawn first", async () => {
  const { engine, calls } = fakeEngine({});
  const { frames } = loader(engine, {
    yieldTask: () => {
      calls.push("yield");
      return Promise.resolve();
    },
  });
  await frames.sync(sync(1, 3));
  assert.deepEqual(calls, [
    "snapshot:3",
    "yield",
    "font:font",
    "yield", // after the font fetch, before compute (the runtime draws pending changes here)
    "compute:a@3",
    "image:img",
    "commit:a",
    "yield",
    "compute:b@3", // the font is loaded: no extra yield
    "image:img",
    "commit:b",
  ]);
});

test("an unavailable font fails its frame; an unavailable image is reported missing", async () => {
  const { engine } = fakeEngine({});
  const { frames, posted } = loader(engine, {
    fetch: (id) => (id === "font" ? Promise.reject(new Error("offline")) : Promise.resolve(new ArrayBuffer(1))),
  });
  await frames.sync(sync(1, 3));
  const first = posted[0] as Extract<WorkerMessage, { type: "frame" }>;
  assert.match(first.error!, /^Missing font: font/);
  const { engine: other } = fakeEngine({});
  const images = loader(other, {
    fetch: (id) => (id === "img" ? Promise.reject(new Error("404")) : Promise.resolve(new ArrayBuffer(1))),
  });
  await images.frames.sync(sync(1, 3));
  const shown = images.posted[0] as Extract<WorkerMessage, { type: "frame" }>;
  assert.deepEqual(shown.missing, ["Missing asset: img"]);
  assert.ok(shown.summary);
});

test("a newer sync supersedes the running one between frames", async () => {
  const { engine, calls } = fakeEngine({});
  let release: () => void = () => {};
  let first = true;
  const { frames, posted } = loader(engine, {
    yieldTask: () =>
      first
        ? new Promise<void>((resolve) => {
            first = false;
            release = resolve;
          })
        : Promise.resolve(),
  });
  const running = frames.sync(sync(1, 3));
  await frames.sync(sync(2, 4));
  release();
  await running;
  assert.ok(!calls.some((call) => call.endsWith("@3")), "revision 3 never computes");
  assert.ok(posted.every((m) => "generation" in m && m.generation === 2));
});

test("a reused frame repeats its summary, so a view that never received it drops the skeleton", async () => {
  const { engine } = fakeEngine({ a: "da", b: "db" });
  const { frames, posted } = loader(engine, {
    fetch: (id) => (id === "img" ? Promise.reject(new Error("404")) : Promise.resolve(new ArrayBuffer(1))),
  });
  await frames.sync(sync(1, 3));
  posted.length = 0;
  // Another scope (or a view replaced before it published) asks for the same frames.
  await frames.sync(sync(2, 4));
  const reused = posted.filter((m) => m.type === "frame") as Extract<WorkerMessage, { type: "frame" }>[];
  assert.deepEqual(
    reused.map((m) => [m.frameId, m.reused, m.revision, m.summary?.revision, m.missing]),
    [
      ["a", true, 4, 4, ["Missing asset: img"]],
      ["b", true, 4, 4, ["Missing asset: img"]],
    ],
  );
});

test("a frame the engine could not commit is reported as failed, never as shown", async () => {
  const { engine } = fakeEngine({});
  (engine as unknown as { commit: () => boolean }).commit = () => false;
  const { frames, posted, renders } = loader(engine);
  await frames.sync(sync(1, 3));
  const first = posted[0] as Extract<WorkerMessage, { type: "frame" }>;
  assert.equal(first.summary, undefined);
  assert.match(first.error!, /released/);
  assert.equal(renders(), 0);
});

test("a layout forgets the frames it releases, and another project starts empty", async () => {
  const { engine } = fakeEngine({ a: "da", b: "db" });
  const { frames } = loader(engine);
  const told = () => [...(frames as unknown as { shown: Map<string, unknown> }).shown.keys()];
  const place = (...ids: string[]) =>
    frames.setLayout(ids.map((id) => ({ id, x: 0, y: 0, width: 100, height: 100 })));
  place("a", "b");
  await frames.sync(sync(1, 3));
  assert.deepEqual(told(), ["a", "b"]);
  // Switching to a page without these frames releases them in the engine and here.
  place("c");
  assert.deepEqual(told(), [], "only placed frames are remembered");
  place("a", "b");
  await frames.sync(sync(2, 3, false));
  assert.deepEqual(told(), ["a", "b"], "released frames compute and are remembered again");
  // Another project's snapshot releases every frame of this one.
  await frames.sync({
    ...sync(3, 1),
    projectId: "q",
    snapshot: { ...snapshot(1), projectId: "q" },
    frames: [],
  });
  assert.deepEqual(told(), [], "a project switch forgets the previous project's frames");
});
