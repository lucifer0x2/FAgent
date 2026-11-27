import assert from "node:assert/strict";
import test from "node:test";
import { FramePacer } from "../src/engine/drawPacing.ts";
import type { EngineApi } from "../src/engine/engineApi.ts";
import { cachedFontBytes, FONT_ENTRIES } from "../src/engine/fontCache.ts";
import { FrameLoader } from "../src/engine/frameLoader.ts";
import type { WorkerMessage } from "../src/engine/protocol.ts";
import {
  evictions,
  type RecordKey,
  type RecordStore,
  type StoredManifest,
  type StoredRecord,
  type WriteBatch,
} from "../src/engine/recordStore.ts";
import { SceneCache } from "../src/engine/sceneCache.ts";
import type { Snapshot } from "../src/types.ts";

const HASH = "a".repeat(64);
const snapshot = (revision: number, projectId = "p"): Snapshot =>
  ({
    projectId,
    slug: "p",
    name: "P",
    revision,
    schemaVersion: 1,
    nodes: [],
    tokens: [],
    styles: [],
    assets: [],
    fonts: [{ id: "font", family: "F", weight: 400, style: "normal", contentHash: HASH }],
    retention: { oldestCommitRevision: 0, idempotencyPolicy: "bounded" },
    truncated: false,
  }) as unknown as Snapshot;

const encode = (value: unknown) => new TextEncoder().encode(JSON.stringify(value));

/**
 * One engine session: records are `{frameId, digest}` JSON; `import_record` refuses bytes
 * that do not parse or answer the request, or a missing font, like the Rust engine.
 */
function fakeEngine(digests: Record<string, string>) {
  const calls: string[] = [];
  const fonts = new Set<string>();
  const shown = new Map<string, string>();
  const staged = new Map<string, string>();
  const summary = (frameId: string, revision: number) =>
    JSON.stringify({ frameId, revision, assets: [], diagnostics: [], height: 10 });
  const engine = {
    load_snapshot: (json: string) => calls.push(`snapshot:${JSON.parse(json).revision}`),
    frame_keys: (_revision: number, frames: string) => {
      calls.push("frame_keys");
      return JSON.stringify(
        JSON.parse(frames).map((f: { frameId: string }) => ({ frameId: f.frameId, digest: digests[f.frameId] })),
      );
    },
    reuse: (id: string, digest: string) => Boolean(digest) && shown.get(id) === digest,
    shows: (id: string, digest: string) => Boolean(digest) && shown.get(id) === digest,
    required_fonts: () => JSON.stringify([{ id: "font", hash: HASH }]),
    has_font: (id: string) => fonts.has(id),
    load_font: (id: string) => {
      fonts.add(id);
      calls.push(`font:${id}`);
    },
    compute: (revision: number, id: string, _w: number, _h: number, digest: string) => {
      calls.push(`compute:${id}`);
      staged.set(id, digest);
      return summary(id, revision);
    },
    import_record: (bytes: Uint8Array, revision: number, id: string, _w: number, _h: number, digest: string) => {
      calls.push(`import:${id}`);
      let record: { frameId?: string; digest?: string };
      try {
        record = JSON.parse(new TextDecoder().decode(bytes));
      } catch {
        throw { code: "RECORD_INVALID", message: "Unreadable frame record" };
      }
      if (record.frameId !== id || record.digest !== digest || !digest)
        throw { code: "RECORD_MISMATCH", message: "Frame record does not answer this request" };
      if (!fonts.has("font")) throw { code: "FONT_MISSING", message: "Font font is not loaded" };
      staged.set(id, digest);
      return summary(id, revision);
    },
    export_record: (id: string, digest: string) => {
      if (shown.get(id) !== digest) throw { code: "RECORD_UNAVAILABLE" };
      calls.push(`export:${id}`);
      return encode({ frameId: id, digest });
    },
    record_fonts: () => JSON.stringify([{ id: "font", hash: HASH }]),
    has_image: () => true,
    add_image: () => undefined,
    commit: (id: string) => {
      if (!staged.has(id)) return false;
      shown.set(id, staged.get(id)!);
      staged.delete(id);
      calls.push(`commit:${id}`);
      return true;
    },
    discard: (id: string) => staged.delete(id),
    set_layout: () => undefined,
  };
  return { engine: engine as unknown as EngineApi, calls };
}

/** An in-memory RecordStore; `hooks` replace its methods' behaviour (failures, gates). */
function memoryStore(
  hooks: {
    read?: (keys: RecordKey[], signal: AbortSignal) => ReturnType<RecordStore["read"]> | undefined;
    write?: (batch: WriteBatch) => Promise<boolean> | undefined;
  } = {},
) {
  const records = new Map<string, StoredRecord>();
  const manifests = new Map<string, StoredManifest>();
  const calls: string[] = [];
  const store: RecordStore = {
    read(keys, signal) {
      calls.push(`read:${keys.length}`);
      const hooked = hooks.read?.(keys, signal);
      if (hooked) return hooked;
      const found = keys.map((key) => Promise.resolve(records.get(JSON.stringify(key))));
      return { records: found, done: Promise.resolve(1) };
    },
    async manifest(key) {
      calls.push("manifest");
      return manifests.get(JSON.stringify(key));
    },
    async write(batch) {
      calls.push(`write:${batch.records.length}:${batch.manifests.length}`);
      const hooked = hooks.write?.(batch);
      if (hooked) return hooked;
      for (const r of batch.records)
        records.set(JSON.stringify([r.build, r.projectId, r.digest, r.width, r.height, r.theme]), r);
      for (const m of batch.manifests)
        manifests.set(JSON.stringify([m.build, m.projectId, m.etag, m.framesKey]), m);
      return true;
    },
    async remove(key) {
      calls.push("remove");
      records.delete(JSON.stringify(key));
    },
  };
  return { store, records, manifests, calls };
}

function session(
  store: RecordStore,
  digests: Record<string, string>,
  options: { build?: string; fetch?: () => Promise<ArrayBuffer> } = {},
) {
  const { engine, calls } = fakeEngine(digests);
  const posted: WorkerMessage[] = [];
  const cache = new SceneCache({
    build: () => options.build ?? "b1",
    open: () => store,
    hash: async (text) => `h${text.length}`,
  });
  const loader = new FrameLoader({
    engine: () => engine,
    post: (message) => posted.push(message),
    requestRender: () => undefined,
    yieldTask: () => new Promise((resolve) => setTimeout(resolve, 0)),
    fetchBytes: options.fetch ?? (() => Promise.resolve(new ArrayBuffer(4))),
    decodeImage: async () => ({ data: new Uint8Array(4), width: 1, height: 1 }),
    cache,
  });
  return { loader, engine, calls, posted, cache };
}

const FRAMES = [
  { frameId: "a", width: 100, height: 100 },
  { frameId: "b", width: 100, height: 100 },
];
const sync = (generation: number, revision: number, extra: Record<string, unknown> = {}) => ({
  type: "sync" as const,
  generation,
  projectId: "p",
  revision,
  snapshot: snapshot(revision),
  frames: FRAMES,
  bypass: false,
  etag: '"objects-1"',
  ...extra,
});
/** Waits until the background writer of `cache` has nothing left. */
async function written(cache: SceneCache) {
  for (let i = 0; i < 200; i++) {
    const stats = cache.stats();
    if (!stats.writing && !stats.pendingWrites) return stats;
    await new Promise((resolve) => setTimeout(resolve, 1));
  }
  throw new Error("cache writes did not finish");
}
const frames = (posted: WorkerMessage[]) =>
  posted.filter((m): m is Extract<WorkerMessage, { type: "frame" }> => m.type === "frame");

test("a reload of the same snapshot imports every frame without frame_keys or compute", async () => {
  const memory = memoryStore();
  const digests = { a: "da", b: "db" };
  const cold = session(memory.store, digests);
  await cold.loader.sync(sync(1, 3));
  assert.deepEqual(cold.calls.filter((c) => /^(frame_keys|compute|import)/.test(c)), [
    "frame_keys",
    "compute:a",
    "compute:b",
  ]);
  const stats = await written(cold.cache);
  assert.equal(stats.written, 2);
  assert.equal(memory.records.size, 2);
  assert.equal(memory.manifests.size, 1);
  // A new session (page reload): same ETag, same frames.
  const warm = session(memory.store, digests);
  await warm.loader.sync(sync(1, 3));
  assert.deepEqual(warm.calls, ["snapshot:3", "font:font", "import:a", "commit:a", "import:b", "commit:b"]);
  assert.deepEqual(
    frames(warm.posted).map((m) => [m.frameId, m.restored, m.summary?.revision]),
    [
      ["a", true, 3],
      ["b", true, 3],
    ],
  );
  const counters = warm.cache.stats();
  assert.deepEqual([counters.manifestHits, counters.hits, counters.misses, counters.imported], [1, 2, 0, 2]);
  assert.equal(memory.calls.filter((c) => c.startsWith("read")).length, 2, "one batched read per sync");
  // A new revision without an ETag: digests from frame_keys, every frame imported.
  const noTag = session(memory.store, digests);
  await noTag.loader.sync(sync(1, 4, { etag: undefined }));
  assert.deepEqual(noTag.calls.filter((c) => /^(frame_keys|compute|import)/.test(c)), ["frame_keys", "import:a", "import:b"]);
});

test("an edited frame recomputes while the others are imported; shown frames are not read", async () => {
  const memory = memoryStore();
  const cold = session(memory.store, { a: "da", b: "db" });
  await cold.loader.sync(sync(1, 3));
  await written(cold.cache);
  const warm = session(memory.store, { a: "da", b: "db2" });
  await warm.loader.sync(sync(1, 4, { etag: '"objects-2"' }));
  assert.deepEqual(warm.calls.filter((c) => /^(frame_keys|compute|import)/.test(c)), ["frame_keys", "import:a", "compute:b"]);
  await written(warm.cache);
  assert.equal(memory.records.size, 3, "the edited frame's record is added");
  // The next revision in the same session: both frames are shown, so nothing is read.
  memory.calls.length = 0;
  await warm.loader.sync(sync(2, 5, { etag: '"objects-3"' }));
  assert.ok(!memory.calls.some((c) => c.startsWith("read")));
  assert.ok(frames(warm.posted).slice(-2).every((m) => m.reused));
});

test("another engine build misses and never reads the old build's records", async () => {
  const memory = memoryStore();
  const cold = session(memory.store, { a: "da", b: "db" });
  await cold.loader.sync(sync(1, 3));
  await written(cold.cache);
  const next = session(memory.store, { a: "da", b: "db" }, { build: "b2" });
  await next.loader.sync(sync(1, 3));
  assert.deepEqual(next.calls.filter((c) => /^(frame_keys|compute|import)/.test(c)), ["frame_keys", "compute:a", "compute:b"]);
  await written(next.cache);
  // A module without a content hash keeps nothing.
  const dev = session(memory.store, { a: "da", b: "db" }, { build: "" });
  memory.calls.length = 0;
  await dev.loader.sync(sync(1, 3));
  assert.deepEqual(memory.calls, []);
  assert.equal(dev.cache.stats().enabled, false);
});

test("a corrupt record is removed and its frame computed, then stored again", async () => {
  const memory = memoryStore();
  const cold = session(memory.store, { a: "da", b: "db" });
  await cold.loader.sync(sync(1, 3));
  await written(cold.cache);
  const [key] = [...memory.records.keys()].filter((k) => k.includes('"da"'));
  memory.records.set(key, { ...memory.records.get(key)!, bytes: new TextEncoder().encode("{not json").buffer as ArrayBuffer });
  const warm = session(memory.store, { a: "da", b: "db" });
  await warm.loader.sync(sync(1, 3));
  assert.deepEqual(warm.calls.filter((c) => /^(compute|import)/.test(c)), ["import:a", "compute:a", "import:b"]);
  assert.ok(memory.calls.includes("remove"));
  assert.equal(warm.cache.stats().rejected, 1);
  assert.ok(frames(warm.posted).every((m) => m.summary && !m.error), "no frame fails");
  await written(warm.cache);
  const again = session(memory.store, { a: "da", b: "db" });
  await again.loader.sync(sync(1, 3));
  assert.deepEqual(again.calls.filter((c) => /^(compute|import)/.test(c)), ["import:a", "import:b"]);
  // A damaged font list beside a record (a font the snapshot does not have) computes too.
  const [first] = [...memory.records.keys()].filter((k) => k.includes("\"da\""));
  memory.records.set(first, { ...memory.records.get(first)!, fonts: [{ id: "font", hash: "0".repeat(64) }] });
  const damaged = session(memory.store, { a: "da", b: "db" });
  await damaged.loader.sync(sync(1, 3));
  assert.deepEqual(damaged.calls.filter((c) => /^(compute|import)/.test(c)), ["compute:a", "import:b"]);
  assert.ok(frames(damaged.posted).every((m) => m.summary && !m.error), "no frame fails");
});

test("a read that times out or a write over quota falls back to compute", async () => {
  // A read whose transaction hit its deadline resolves every record as a miss.
  const timedOut = memoryStore({
    read: (keys) => ({ records: keys.map(() => Promise.resolve(undefined)), done: Promise.resolve(3_000) }),
    write: () => Promise.resolve(false),
  });
  const first = session(timedOut.store, { a: "da", b: "db" });
  await first.loader.sync(sync(1, 3));
  await written(first.cache);
  const warm = session(timedOut.store, { a: "da", b: "db" });
  await warm.loader.sync(sync(1, 3));
  assert.deepEqual(warm.calls.filter((c) => /^(frame_keys|compute|import)/.test(c)), ["frame_keys", "compute:a", "compute:b"]);
  assert.ok(frames(warm.posted).every((m) => m.summary && !m.error));
  const stats = await written(warm.cache);
  assert.ok(stats.writeFailures >= 1);
  assert.equal(stats.written, 0);
});

test("Retry bypasses cache reads and replaces the records it recomputes", async () => {
  const memory = memoryStore();
  const cold = session(memory.store, { a: "da", b: "db" });
  await cold.loader.sync(sync(1, 3));
  await written(cold.cache);
  const warm = session(memory.store, { a: "da", b: "db" });
  memory.calls.length = 0;
  await warm.loader.sync(sync(1, 3, { bypass: true }));
  assert.ok(!memory.calls.some((c) => c === "manifest" || c.startsWith("read")), JSON.stringify(memory.calls));
  assert.deepEqual(warm.calls.filter((c) => /^(frame_keys|compute|import)/.test(c)), ["frame_keys", "compute:a", "compute:b"]);
  await written(warm.cache);
  assert.ok(memory.calls.includes("write:2:1"), "both records and the manifest are written again");
});

test("a newer sync cancels the pending imports of the one it replaces", async () => {
  const memory = memoryStore();
  const cold = session(memory.store, { a: "da", b: "db" });
  await cold.loader.sync(sync(1, 3));
  await written(cold.cache);
  let open = () => {};
  const gate = new Promise<void>((resolve) => (open = resolve));
  const signals: AbortSignal[] = [];
  const gated = memoryStore({
    read: (keys, signal) => {
      signals.push(signal);
      const records = keys.map(async (key) => {
        await gate;
        return signal.aborted ? undefined : memory.records.get(JSON.stringify(key));
      });
      return { records, done: gate.then(() => 1) };
    },
  });
  for (const [k, v] of memory.manifests) gated.manifests.set(k, v);
  const warm = session(gated.store, { a: "da", b: "db" });
  const older = warm.loader.sync(sync(1, 3));
  await new Promise((resolve) => setTimeout(resolve, 5));
  const newer = warm.loader.sync(sync(2, 4, { etag: undefined }));
  open();
  await Promise.all([older, newer]);
  assert.ok(signals[0].aborted, "the replaced sync's read is aborted");
  assert.ok(warm.posted.every((m) => !("generation" in m) || m.generation === 2));
  assert.deepEqual(warm.calls.filter((c) => /^(compute|import)/.test(c)), ["import:a", "import:b"]);
});

test("nothing is read from the cache before a sync carries a snapshot the server answered", async () => {
  const memory = memoryStore();
  const fresh = session(memory.store, { a: "da", b: "db" });
  // A sync for a revision this worker never received (no snapshot attached).
  await fresh.loader.sync(sync(1, 3, { snapshot: undefined }));
  // A snapshot that does not match its request.
  await fresh.loader.sync(sync(2, 4, { snapshot: snapshot(5) }));
  assert.deepEqual(memory.calls, []);
  assert.deepEqual(
    fresh.posted.map((m) => m.type === "synced" && Boolean(m.error)),
    [true, true],
  );
  await fresh.loader.sync(sync(3, 4));
  assert.deepEqual(memory.calls.slice(0, 2), ["manifest", "read:2"]);
});

test("a record's fonts are fetched before it is imported; an unavailable font fails the frame", async () => {
  const memory = memoryStore();
  const cold = session(memory.store, { a: "da", b: "db" });
  await cold.loader.sync(sync(1, 3));
  await written(cold.cache);
  const warm = session(memory.store, { a: "da", b: "db" });
  await warm.loader.sync(sync(1, 3));
  assert.deepEqual(warm.calls.slice(1, 3), ["font:font", "import:a"]);
  const offline = session(memory.store, { a: "da", b: "db" }, { fetch: () => Promise.reject(new Error("offline")) });
  await offline.loader.sync(sync(1, 3));
  assert.match(frames(offline.posted)[0].error!, /^Missing font: font/);
  assert.ok(!offline.calls.some((c) => c.startsWith("compute") || c.startsWith("import")));
});

test("eviction drops least recently used rows; a replaced key is not counted twice", () => {
  const key = (d: string): RecordKey => ["b1", "p", d, 100, 100, "default"];
  const rows = [
    { key: key("old"), size: 10, used: 1 },
    { key: key("mid"), size: 10, used: 2 },
    { key: key("new"), size: 10, used: 3 },
  ];
  assert.deepEqual(evictions(rows, [{ key: key("x"), size: 10 }], { entries: 3, bytes: 1_000 }), [key("old")]);
  assert.deepEqual(evictions(rows, [{ key: key("x"), size: 25 }], { entries: 10, bytes: 50 }), [key("old")]);
  assert.deepEqual(evictions(rows, [{ key: key("x"), size: 25 }], { entries: 10, bytes: 40 }), [key("old"), key("mid")]);
  assert.deepEqual(evictions(rows, [{ key: key("mid"), size: 10 }], { entries: 3, bytes: 1_000 }), []);
});

test("restored frames wait for the gap or the end of the sync; a computed one draws at once", () => {
  let clock = 0;
  const posted: WorkerMessage[] = [];
  const pacer = new FramePacer(() => clock, (m) => posted.push(m));
  const frame = (restored: boolean) =>
    ({ type: "frame", generation: 1, frameId: "f", revision: 1, ...(restored ? { restored } : {}) }) as WorkerMessage;
  pacer.drew(16);
  pacer.startSync();
  pacer.hold(frame(true));
  clock = 100;
  pacer.hold(frame(true));
  assert.equal(pacer.due(), false, "restored frames do not trigger the first-frame draw");
  clock = 160;
  assert.equal(pacer.due(), true, "the gap counts from the first held frame");
  pacer.release();
  pacer.startSync();
  pacer.hold(frame(true));
  pacer.hold(frame(false));
  assert.equal(pacer.due(), true, "a computed frame is drawn at once, with the restored ones");
});

test("font bytes come from the worker cache once verified; a damaged entry is fetched again", async () => {
  const entries = new Map<string, ArrayBuffer>();
  const cache = {
    match: async (url: string) => (entries.has(url) ? new Response(entries.get(url)!.slice(0)) : undefined),
    put: async (url: string, response: Response) => void entries.set(url, await response.arrayBuffer()),
    delete: async (url: string) => entries.delete(url),
    keys: async () => [...entries.keys()].map((url) => new Request(url)),
  };
  const storage = { open: async () => cache } as unknown as CacheStorage;
  const bytes = new TextEncoder().encode("font bytes").buffer as ArrayBuffer;
  const hash = Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)), (b) =>
    b.toString(16).padStart(2, "0"),
  ).join("");
  let fetches = 0;
  const fetchFont = cachedFontBytes(
    async () => {
      fetches++;
      return bytes;
    },
    { enabled: () => true, storage, origin: "http://127.0.0.1" },
  );
  const manifest = { id: "font", hash, url: "/fonts/font" };
  await fetchFont(manifest);
  await new Promise((resolve) => setTimeout(resolve, 1));
  assert.deepEqual(new Uint8Array(await fetchFont(manifest)), new Uint8Array(bytes));
  assert.equal(fetches, 1, "the second load reads the cache");
  const [url] = entries.keys();
  entries.set(url, new TextEncoder().encode("damaged").buffer as ArrayBuffer);
  assert.deepEqual(new Uint8Array(await fetchFont(manifest)), new Uint8Array(bytes));
  assert.equal(fetches, 2, "a damaged entry is fetched again");
  const off = cachedFontBytes(async () => (fetches++, bytes), { enabled: () => false, storage, origin: "http://127.0.0.1" });
  await off(manifest);
  assert.equal(fetches, 3);
  assert.ok(FONT_ENTRIES >= 16);
});
