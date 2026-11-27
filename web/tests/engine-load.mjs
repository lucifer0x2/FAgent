// Cold-load benchmark for renderer=vello on a large project (Task 36 item 1): a temporary
// native backend (SQLite in a temp dir, loopback port) seeded through MCP with
// engine-load-fixture.mjs, a production build of this checkout, Chromium with a real GPU,
// 1440×900 at DPR 2, Layers closed. Each run is a new browser context (cold page, warm HTTP
// cache off). Marks come from the page clock (navigation = 0): Worker created, `/snapshot`
// request started, `sync` sent, first frame message, first `drawn` after it, `synced`, first
// `drawn` after `synced`. Worker-side engine method times and the times of `compute`,
// `commit`, `render` and `set_camera` come from wrapping the wasm-bindgen glue's
// `Engine.prototype` through a route, so the product carries no probe; they are moved onto
// the page clock through both `performance.timeOrigin`s.
// Usage: node web/tests/engine-load.mjs [--frames 225] [--pages 10] [--runs 3]
//   [--scopes all,pg-0] [--latency <ms>] [--channel chromium|chrome] [--dist <dir>] [--label <name>]
//   [--interact]   (zoom while loading; reports camera order and camera → drawn latency)
//   [--warm [--edit <k> | --edit-shared]]
// --warm (Task 42): per scope one persistent profile in a temporary directory; a first
// (cold) navigation fills the engine scene cache, then `--runs` reloads of the same page are
// measured. `--edit k` retitles k screens through MCP before each reload (partial
// invalidation); `--edit-shared` changes the token every root frame uses. Warm runs also
// report engine call counts (`frame_keys`, `compute`, `import_record`, …) and the worker's
// cache counters (hits, misses, bytes and time read); the runner waits for pending cache
// writes before the next navigation.
// Writes artifacts/engine-load/summary.json (ignored); everything temporary is removed.
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { connectMcp } from "../../spikes/e2e/protocol.mjs";
import { editFrames, editShared, seedLargeProject } from "./engine-load-fixture.mjs";
import { startBackend } from "./repaint-fixture.mjs";
import { serveViewer } from "./repaint-viewer.mjs";
import { launch, LONG_TASKS } from "./engine-support.mjs";
import { LOAD_PROBE, ENGINE_TIMES, summarize, aggregate } from "./engine-load-probe.mjs";

const REPO = fileURLToPath(new URL("../../", import.meta.url));
const OUT = join(REPO, "artifacts/engine-load");
const { values: args } = parseArgs({
  options: {
    frames: { type: "string", default: "225" },
    pages: { type: "string", default: "10" },
    runs: { type: "string", default: "3" },
    scopes: { type: "string", default: "all" },
    latency: { type: "string", default: "0" },
    channel: { type: "string", default: process.env.FAGENT_TEST_CHANNEL ?? "chromium" },
    /** A prebuilt viewer dist (e.g. of another commit, for a before/after comparison). */
    dist: { type: "string" },
    /** Writes summary-<label>.json instead of summary.json. */
    label: { type: "string" },
    /** Zooms during the load and reports camera → drawn latency (message order check). */
    interact: { type: "boolean", default: false },
    /** Reloads in one persistent profile after a cold fill (engine scene cache). */
    warm: { type: "boolean", default: false },
    /** With --warm: screens retitled through MCP before each reload. */
    edit: { type: "string", default: "0" },
    /** With --warm: the shared token changes before each reload (every frame recomputes). */
    "edit-shared": { type: "boolean", default: false },
  },
});
/** `--name` as an integer of at least `min`; anything else stops before seeding. */
function integer(name, min) {
  const value = Number(args[name]);
  if (!Number.isInteger(value) || value < min)
    throw new Error(`--${name} must be an integer ≥ ${min}, got ${JSON.stringify(args[name])}`);
  return value;
}
const counts = {
  frames: integer("frames", 1),
  pages: integer("pages", 1),
  runs: integer("runs", 1),
  latency: integer("latency", 0),
  edit: integer("edit", 0),
};
if (counts.pages > counts.frames)
  throw new Error(`--pages ${counts.pages} needs at least as many --frames (every page gets one)`);
if ((counts.edit || args["edit-shared"]) && !args.warm)
  throw new Error("--edit and --edit-shared need --warm");
if (counts.edit && args["edit-shared"]) throw new Error("pass --edit or --edit-shared, not both");

async function buildDist() {
  const require = createRequire(join(REPO, "web/package.json"));
  const { build } = await import(require.resolve("vite"));
  const outDir = join(OUT, "dist");
  await build({
    root: join(REPO, "web"),
    configFile: join(REPO, "web/vite.config.ts"),
    logLevel: "warn",
    build: { outDir, emptyOutDir: true },
  });
  return outDir;
}

const VIEW = { viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 };

/** Init scripts and routes every measured context needs (cold or persistent). */
async function prepare(context) {
  await context.addInitScript(LONG_TASKS);
  await context.addInitScript(LOAD_PROBE);
  await context.addInitScript(() => localStorage.setItem("left-open", "false"));
  // Wrap the engine methods in the worker's copy of the glue (hash-named module). Routing
  // turns the browser HTTP cache off for this context (Playwright), cold and warm alike.
  await context.route(/\/engine-wasm\/fagent_engine_wasm-[0-9a-f]+\.js$/, async (route) => {
    const response = await route.fetch();
    route.fulfill({ response, body: (await response.text()) + ENGINE_TIMES });
  });
  const latency = counts.latency;
  if (latency > 0)
    await context.route(/\/api\/v1\/(fonts|projects\/[^/]+\/(fonts|assets))\//, async (route) => {
      await new Promise((resolve) => setTimeout(resolve, latency));
      await route.continue();
    });
}

/** One navigation of `page` to the board of `scope`, measured until synced and drawn. */
async function load(page, base, project, scope) {
  page.setDefaultTimeout(180_000);
  const errors = [];
  const onError = (error) => errors.push(error.message);
  page.on("pageerror", onError);
  const query = new URLSearchParams({
    project: project.projectId,
    page: scope === "all" ? ":all:" : scope,
    frame: ":all:",
    renderer: "vello",
    engineDebug: "1",
  });
  await page.goto(`${base}/?${query}`);
  if (args.interact) {
    // Zoom while frames compute: each camera must be drawn before the next frame starts,
    // not after the load (worker call order; see cameraOrder and cameraToDrawn).
    await page.waitForFunction(() => window.__load.events.some((e) => e[0] === "frame"));
    const box = await page.locator(".canvas-host").boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    for (let i = 0; i < 6; i++) {
      await page.mouse.wheel(0, i % 2 ? 60 : -60);
      await page.waitForTimeout(150);
    }
  }
  const expected = scope === "all" ? project.rootFrames : null;
  try {
    await page.waitForFunction(
      (expected) => window.__load.done(expected),
      expected,
      { timeout: 120_000, polling: 100 },
    );
  } catch (cause) {
    const events = await page.evaluate(() => window.__load.events.filter((e) => e[0] !== "frame"));
    const status = await page.locator(".status-bar, footer").allInnerTexts().catch(() => []);
    throw new Error(`load did not finish: ${JSON.stringify({ events, status, errors }).slice(0, 4000)}`, { cause });
  }
  // Let the final draw land and the main thread settle.
  await page.waitForTimeout(400);
  const { events, origin } = await page.evaluate(() => ({
    events: window.__load.events,
    origin: performance.timeOrigin,
  }));
  const longTasks = (await page.evaluate(() => window.__longTasks)) ?? [];
  const worker = page.workers().find((w) => w.url().includes("engine"));
  const engine = worker
    ? await worker.evaluate(() => ({
        times: globalThis.__engineTimes ?? null,
        log: globalThis.__engineLog ?? [],
        origin: performance.timeOrigin,
        // Font fetches of the worker (Resource Timing): start and end, worker clock.
        fonts: performance
          .getEntriesByType("resource")
          .filter((e) => /\/fonts\//.test(new URL(e.name).pathname))
          .map((e) => [e.startTime, e.responseEnd]),
      }))
    : null;
  const status = await page.evaluate(() => window.fagentEngine?.status());
  page.off("pageerror", onError);
  // Worker clock → page clock: both count from their own time origin.
  const shift = engine ? engine.origin - origin : 0;
  const log = (engine?.log ?? []).map(([name, start, end, arg]) => [name, start + shift, end + shift, arg]);
  // Font work of the load: first font request to the last `load_font` (fetch, SHA-256 check,
  // engine load), and the summed request time.
  const fetches = engine?.fonts ?? [];
  const loads = log.filter((c) => c[0] === "load_font");
  const fonts = fetches.length
    ? {
        count: fetches.length,
        windowMs: Math.round(Math.max(...loads.map((c) => c[2]), ...fetches.map((f) => f[1] + shift)) - (Math.min(...fetches.map((f) => f[0])) + shift)),
        fetchMs: Math.round(fetches.reduce((sum, [start, end]) => sum + end - start, 0)),
      }
    : { count: 0, windowMs: 0, fetchMs: 0 };
  return {
    ...summarize(events, longTasks, engine?.times, log),
    fonts,
    backend: status?.backend,
    cache: cacheStats(status?.cache),
    errors,
  };
}

/** Numeric cache counters of the worker's status (none before Task 42: warm = cold). */
function cacheStats(cache) {
  if (!cache) return undefined;
  return Object.fromEntries(Object.entries(cache).filter(([, v]) => typeof v === "number"));
}

/**
 * Waits until the worker has written what the last load computed (its status reports no
 * pending cache work), so the next navigation finds it; a build without a cache is idle.
 */
async function cacheIdle(page) {
  const deadline = Date.now() + 60_000;
  for (;;) {
    const cache = await page.evaluate(async () => (await window.fagentEngine?.status())?.cache);
    if (!cache || (cache.pendingWrites === 0 && !cache.writing)) return cache;
    if (Date.now() > deadline) throw new Error(`cache writes did not finish: ${JSON.stringify(cache)}`);
    await page.waitForTimeout(100);
  }
}

/** Cold: a new browser context (empty profile) per run. */
async function run(browser, base, project, scope) {
  const context = await browser.newContext(VIEW);
  try {
    await prepare(context);
    return await load(await context.newPage(), base, project, scope);
  } finally {
    await context.close();
  }
}

/**
 * Warm: one persistent profile in a temporary directory. The first navigation is cold and
 * fills the cache; each further one reloads the same page (after `--edit`/`--edit-shared`
 * changed the project through MCP while the page was away, so no live update recomputes
 * the edited frames first).
 */
async function series(base, project, scope, mcp) {
  const { chromium } = await import("playwright");
  const dir = await mkdtemp(join(tmpdir(), "fagent-engine-warm-"));
  const context = await chromium.launchPersistentContext(dir, {
    channel: args.channel,
    headless: true,
    args: ["--force-color-profile=srgb"],
    ...VIEW,
  });
  try {
    await prepare(context);
    const page = context.pages()[0] ?? (await context.newPage());
    const fill = await load(page, base, project, scope);
    fill.written = await cacheIdle(page);
    console.error(`${scope} fill: synced ${fill.marks.synced} ms, computed ${fill.calls.compute}`);
    const runs = [];
    for (let i = 0; i < counts.runs; i++) {
      if (counts.edit || args["edit-shared"]) {
        await page.goto("about:blank");
        project.revision = args["edit-shared"]
          ? await editShared(mcp, project, project.revision, i + 1)
          : await editFrames(mcp, project, project.revision, counts.edit, i + 1);
      }
      const result = await load(page, base, project, scope);
      result.written = await cacheIdle(page);
      console.error(
        `${scope} warm ${i + 1}: synced ${result.marks.synced} ms (after backend ${result.syncedAfterBackend}), ` +
          `frame_keys ${result.calls.frame_keys}, compute ${result.calls.compute}, import ${result.calls.import_record}`,
      );
      runs.push(result);
    }
    return { browser: context.browser()?.version(), fill, runs };
  } finally {
    await context.close();
    await rm(dir, { recursive: true, force: true });
  }
}

await mkdir(OUT, { recursive: true });
const backend = await startBackend();
let viewer, browser, mcp;
try {
  const started = Date.now();
  const project = await seedLargeProject(backend.base, new URL("../../", import.meta.url), {
    frames: counts.frames,
    pages: counts.pages,
  });
  console.error(`seeded ${project.nodes} nodes in ${Date.now() - started} ms`);
  const dist = args.dist ?? (await buildDist());
  viewer = await serveViewer(REPO, dist, backend.base);
  process.env.FAGENT_TEST_CHANNEL = args.channel;
  const scopes = args.scopes.split(",").map((s) => (s === "all" ? "all" : s));
  const results = {};
  let version;
  if (args.warm) {
    mcp = await connectMcp(backend.base);
    for (const scope of scopes) {
      const warm = await series(viewer.url, project, scope, mcp);
      version = warm.browser ?? version;
      results[scope] = { fill: warm.fill, ...aggregate(warm.runs), runs: warm.runs };
    }
  } else {
    browser = await launch({ gpu: true });
    version = browser.version();
    for (const scope of scopes) {
      const runs = [];
      for (let i = 0; i < counts.runs; i++) {
        const result = await run(browser, viewer.url, project, scope);
        console.error(`${scope} run ${i + 1}: synced ${result.marks.synced} ms, drawn ${result.marks.lastDrawn} ms`);
        runs.push(result);
      }
      results[scope] = { ...aggregate(runs), runs };
    }
  }
  const summary = {
    fixture: { nodes: project.nodes, instances: project.instances, frames: project.rootFrames, pages: project.pages, fonts: project.fonts },
    browser: `${args.channel} ${version}`,
    dpr: 2,
    viewport: "1440x900",
    latencyMs: counts.latency,
    mode: args.warm
      ? { warm: true, edit: counts.edit, editShared: args["edit-shared"] }
      : { warm: false },
    results,
  };
  const file = args.label ? `summary-${args.label}.json` : "summary.json";
  await writeFile(join(OUT, file), JSON.stringify(summary, null, 2) + "\n");
  console.log(JSON.stringify({ fixture: summary.fixture, browser: summary.browser }));
  for (const [scope, r] of Object.entries(results))
    console.log(
      JSON.stringify({
        scope,
        p50: Object.fromEntries(Object.entries(r.marks).map(([k, v]) => [k, v?.p50 ?? null])),
        draws: r.drawsWhileLoading?.p50,
        commitToDrawnP95: r.commitToDrawnP95?.p50,
        commitToDrawnMax: r.commitToDrawnMax?.max,
        cameraToDrawnMax: r.cameraToDrawnMax?.max,
        cameraOrderViolations: r.cameraOrderViolations,
        longTasksMs: r.longTasksTotal?.p50,
        fontWindowMs: r.fontWindowMs?.p50,
        engineMs: r.engineMsP50,
        ...(args.warm
          ? {
              fillSynced: r.fill.marks.synced,
              fillAfterBackend: r.fill.syncedAfterBackend,
              afterBackend: r.syncedAfterBackend,
              calls: Object.fromEntries(Object.entries(r.calls).map(([k, v]) => [k, v?.p50])),
              cache: Object.fromEntries(Object.entries(r.cache).map(([k, v]) => [k, v?.p50])),
            }
          : {}),
      }),
    );
} finally {
  await mcp?.close();
  await browser?.close();
  await viewer?.stop();
  await backend.stop();
  if (!args.dist) await rm(join(OUT, "dist"), { recursive: true, force: true });
}
