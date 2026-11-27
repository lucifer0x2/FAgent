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
// Writes artifacts/engine-load/summary.json (ignored); everything temporary is removed.
import { mkdir, rm, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { seedLargeProject } from "./engine-load-fixture.mjs";
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
};
if (counts.pages > counts.frames)
  throw new Error(`--pages ${counts.pages} needs at least as many --frames (every page gets one)`);

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

async function run(browser, base, project, scope) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });
  await context.addInitScript(LONG_TASKS);
  await context.addInitScript(LOAD_PROBE);
  await context.addInitScript(() => localStorage.setItem("left-open", "false"));
  // Wrap the engine methods in the worker's copy of the glue (hash-named module).
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
  const page = await context.newPage();
  page.setDefaultTimeout(180_000);
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
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
      }))
    : null;
  const status = await page.evaluate(() => window.fagentEngine?.status());
  await context.close();
  // Worker clock → page clock: both count from their own time origin.
  const shift = engine ? engine.origin - origin : 0;
  const log = (engine?.log ?? []).map(([name, start, end, arg]) => [name, start + shift, end + shift, arg]);
  return { ...summarize(events, longTasks, engine?.times, log), backend: status?.backend, errors };
}

await mkdir(OUT, { recursive: true });
const backend = await startBackend();
let viewer, browser;
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
  browser = await launch({ gpu: true });
  const scopes = args.scopes.split(",").map((s) => (s === "all" ? "all" : s));
  const results = {};
  for (const scope of scopes) {
    const runs = [];
    for (let i = 0; i < counts.runs; i++) {
      const result = await run(browser, viewer.url, project, scope);
      console.error(`${scope} run ${i + 1}: synced ${result.marks.synced} ms, drawn ${result.marks.lastDrawn} ms`);
      runs.push(result);
    }
    results[scope] = { ...aggregate(runs), runs };
  }
  const summary = {
    fixture: { nodes: project.nodes, instances: project.instances, frames: project.rootFrames, pages: project.pages, fonts: project.fonts },
    browser: `${args.channel} ${browser.version()}`,
    dpr: 2,
    viewport: "1440x900",
    latencyMs: counts.latency,
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
        engineMs: r.engineMsP50,
      }),
    );
} finally {
  await browser?.close();
  await viewer?.stop();
  await backend.stop();
  if (!args.dist) await rm(join(OUT, "dist"), { recursive: true, force: true });
}
