// The engine scene cache's IndexedDB store (web/src/engine/recordStore.ts) in a real
// Chromium (Task 42): one read transaction resolving each record on its own, misses and
// damaged values as undefined, other engine builds removed on write, least recently used
// eviction (touched records stay), manifests, removal and an aborted read. The module runs
// through Vite on a blank page of its origin, in a new browser context (empty profile).
// Usage: node web/tests/engine-cache-browser.mjs   (FAGENT_TEST_CHANNEL=chrome for Chrome)
import assert from "node:assert/strict";
import { launch, startVite } from "./engine-support.mjs";

const vite = await startVite("http://127.0.0.1:9");
const browser = await launch({ gpu: false });
try {
  const context = await browser.newContext();
  const page = await context.newPage();
  await page.route("**/__blank", (route) =>
    route.fulfill({ contentType: "text/html", body: "<!doctype html><title>cache</title>" }),
  );
  await page.goto(`${vite.base}/__blank`);
  const result = await page.evaluate(async () => {
    const m = await import("/src/engine/recordStore.ts");
    const bytes = (text) => new TextEncoder().encode(text).buffer;
    const record = (build, digest, size = 10) => ({
      build, projectId: "p", digest, width: 100, height: 100, theme: m.THEME, frameId: `f-${digest}`,
      bytes: bytes(`record ${digest}`), fonts: [{ id: "font", hash: "h" }], assets: [], size,
    });
    const key = (build, digest) => [build, "p", digest, 100, 100, m.THEME];
    const read = async (store, keys, signal = new AbortController().signal) => {
      const { records, done } = store.read(keys, signal);
      const values = await Promise.all(records);
      await done;
      return values.map((v) => (v ? new TextDecoder().decode(v.bytes) : null));
    };
    const out = {};
    const one = m.indexedDbStore("b1", { entries: 3, bytes: 1_000 });
    out.wrote = await one.write({
      records: ["a", "b", "c"].map((d) => record("b1", d)),
      touched: [],
      manifests: [{ build: "b1", projectId: "p", etag: '"e"', framesKey: "k", digests: { f: "a" }, used: 0 }],
    });
    out.read = await read(one, [key("b1", "a"), key("b1", "zz"), key("b1", "c")]);
    out.manifest = (await one.manifest(["b1", "p", '"e"', "k"], new AbortController().signal))?.digests;
    // Touch "a" (read by a load), then add "d": the least recently used ("b") goes.
    await new Promise((resolve) => setTimeout(resolve, 5));
    await one.write({ records: [], touched: [{ key: key("b1", "a"), size: 10 }], manifests: [] });
    await new Promise((resolve) => setTimeout(resolve, 5));
    await one.write({ records: [record("b1", "d")], touched: [], manifests: [] });
    out.afterEviction = await read(one, ["a", "b", "c", "d"].map((d) => key("b1", d)));
    await one.remove(key("b1", "c"));
    out.afterRemove = await read(one, [key("b1", "c")]);
    // A damaged value (no bytes) reads as a miss.
    await new Promise((resolve, reject) => {
      const open = indexedDB.open("fagent.engine-scenes");
      open.onsuccess = () => {
        const tx = open.result.transaction("records", "readwrite");
        tx.objectStore("records").put({ ...record("b1", "d"), bytes: "not bytes" });
        tx.oncomplete = () => (open.result.close(), resolve());
        tx.onerror = () => reject(tx.error);
      };
    });
    out.damaged = await read(one, [key("b1", "d")]);
    // An aborted read resolves every record as a miss.
    const controller = new AbortController();
    controller.abort();
    out.aborted = await read(one, [key("b1", "a")], controller.signal);
    // Another build's write removes every row of b1 (records, usage, manifests).
    const two = m.indexedDbStore("b2");
    await two.write({ records: [record("b2", "a")], touched: [], manifests: [] });
    out.oldBuild = await read(one, [key("b1", "a")]);
    out.oldManifest = await one.manifest(["b1", "p", '"e"', "k"], new AbortController().signal);
    out.newBuild = await read(two, [key("b2", "a")]);
    return out;
  });
  assert.equal(result.wrote, true);
  assert.deepEqual(result.read, ["record a", null, "record c"]);
  assert.deepEqual(result.manifest, { f: "a" });
  assert.deepEqual(result.afterEviction, ["record a", null, "record c", "record d"]);
  assert.deepEqual(result.afterRemove, [null]);
  assert.deepEqual(result.damaged, [null]);
  assert.deepEqual(result.aborted, [null]);
  assert.deepEqual(result.oldBuild, [null]);
  assert.equal(result.oldManifest, undefined);
  assert.deepEqual(result.newBuild, ["record a"]);
  await context.close();
  console.log("engine-cache-browser: ok");
} finally {
  await browser.close();
  await vite.close();
}
