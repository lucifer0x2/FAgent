// Board benchmark for renderer=vello vs Canvas2D (Task 30): the 12-frame / 4 944-node page
// of repaint-fixture.mjs on a temporary native backend (SQLite in a temp dir, loopback port),
// a production build of this checkout, Chromium full build in new headless mode (real GPU),
// 1440×900 at DPR 2. Measures cold load, main-thread long tasks while loading, panning and
// zooming (PerformanceObserver), and the engine's worker-side draw times at Fit and 100%.
// Usage: node web/tests/engine-board.mjs [--renderers vello,canvas2d] [--layers open,closed] [--strict]
//   --strict fails when a vello main-thread long task exceeds 50 ms with the Layers panel
//   closed (the panel renders one row per node on the main thread for either renderer).
// Writes artifacts/engine-board/summary.json (ignored); everything temporary is removed.
import { mkdir, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { createRequire } from "node:module";
import { seedProject, startBackend } from "./repaint-fixture.mjs";
import { serveViewer } from "./repaint-viewer.mjs";
import { launch, LONG_TASKS } from "./engine-support.mjs";

const REPO = fileURLToPath(new URL("../../", import.meta.url));
const OUT = join(REPO, "artifacts/engine-board");
const { values: args } = parseArgs({
  options: {
    // "vello-atlas": renderer=vello with the experimental glyph atlas (?glyphAtlas=1).
    renderers: { type: "string", default: "canvas2d,vello,vello-atlas" },
    layers: { type: "string", default: "open,closed" },
    strict: { type: "boolean", default: false },
  },
});
const pct = (values, p) => {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return Math.round(sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))] * 10) / 10;
};

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

const phase = (durations) => ({
  count: durations.length,
  maxMs: durations.length ? Math.max(...durations) : 0,
  over50: durations.filter((d) => d > 50).length,
});

async function pan(page, steps) {
  const box = await page.locator(".canvas-host").boundingBox();
  const x = box.x + box.width / 2,
    y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  for (let i = 1; i <= steps; i++) {
    await page.mouse.move(x - (i % 30) * 12 + 180, y - (i % 30) * 6 + 90);
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(r)));
  }
  await page.mouse.up();
}

/** Worker draw times (encode + submit, worker clock) between two worker-clock marks. */
function draws(status, from, to) {
  const renders = status.renders.filter(([at]) => at >= from && at < to);
  const draw = renders.map(([, ms]) => ms);
  const gaps = renders.slice(1).map(([at], i) => at - renders[i][0]);
  return {
    renders: renders.length,
    drawP50: pct(draw, 0.5),
    drawP95: pct(draw, 0.95),
    drawMax: draw.length ? Math.max(...draw) : null,
    intervalP50: pct(gaps, 0.5),
    intervalP95: pct(gaps, 0.95),
  };
}

async function run(browser, base, projectId, revision, renderer, layers) {
  const vello = renderer.startsWith("vello");
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    deviceScaleFactor: 2,
  });
  await context.addInitScript(LONG_TASKS);
  // The Layers panel renders one row per node (4 945 here) on the main thread for either
  // renderer; "closed" isolates the canvas path from that app-shell cost.
  if (layers === "closed")
    await context.addInitScript(() => localStorage.setItem("left-open", "false"));
  const page = await context.newPage();
  page.setDefaultTimeout(180_000);
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const query = new URLSearchParams({
    project: projectId,
    page: "page",
    frame: ":all:",
    renderer: vello ? "vello" : renderer,
    ...(renderer === "vello-atlas" ? { glyphAtlas: "1" } : {}),
    engineDebug: "1",
  });
  // Page clock and (vello) worker clock at each phase boundary.
  const marks = [];
  const mark = async () =>
    marks.push({
      page: await page.evaluate(() => performance.now()),
      worker: vello
        ? await page.evaluate(() => window.fagentEngine.status().then((s) => s.now))
        : 0,
    });
  const started = Date.now();
  await page.goto(`${base}/?${query}`);
  await page.getByText(`Scene r${revision}`, { exact: true }).waitFor();
  const loadMs = Date.now() - started;
  await page.waitForTimeout(1500);
  await mark(); // 0: loaded, at Fit
  const fit = vello
    ? (await page.evaluate(() => window.fagentEngine.status())).lastRender
    : null;
  await pan(page, 90);
  await page.waitForTimeout(300);
  await mark(); // 1
  const box = await page.locator(".canvas-host").boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  for (let i = 0; i < 24; i++) {
    await page.mouse.wheel(0, i < 12 ? -40 : 40);
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(r)));
  }
  await page.waitForTimeout(300);
  await mark(); // 2
  // 100%: Fit is about 0.14 here; eleven keyboard steps of 1.2 reach ~1.
  await page.locator(".canvas-host").focus();
  for (let i = 0; i < 11; i++) await page.keyboard.press("+");
  await page.waitForTimeout(500);
  await mark(); // 3
  const at100 = vello
    ? (await page.evaluate(() => window.fagentEngine.status())).lastRender
    : null;
  await pan(page, 90);
  await page.waitForTimeout(300);
  await mark(); // 4
  const tasks = (await page.evaluate(() => window.__longTasks)) ?? [];
  const between = (a, b) =>
    phase(
      tasks
        .filter((t) => t.start >= (a < 0 ? 0 : marks[a].page) && t.start < marks[b].page)
        .map((t) => t.duration),
    );
  const result = {
    renderer,
    layers,
    loadMs,
    longTasks: {
      load: between(-1, 0),
      panFit: between(0, 1),
      zoom: between(1, 2),
      pan100: between(3, 4),
      all: phase(tasks.map((t) => t.duration)),
    },
    zoomLabel: (await page.locator(".zoom-controls").innerText()).replace(/\s+/g, " ").trim(),
    errors,
  };
  if (vello) {
    const status = await page.evaluate(() => window.fagentEngine.status());
    result.engine = {
      backend: status.backend,
      ...(await page.evaluate(() => {
        const { loadMs, initMs } = window.fagentEngine.state();
        return { moduleLoadMs: loadMs, backendInitMs: initMs };
      })),
      fit,
      at100,
      panFit: draws(status, marks[0].worker, marks[1].worker),
      zoom: draws(status, marks[1].worker, marks[2].worker),
      pan100: draws(status, marks[3].worker, marks[4].worker),
    };
  }
  await context.close();
  return result;
}

await mkdir(OUT, { recursive: true });
const backend = await startBackend();
let viewer, browser, project;
try {
  project = await seedProject(backend.base);
  const dist = await buildDist();
  viewer = await serveViewer(REPO, dist, backend.base);
  browser = await launch({ gpu: true });
  const results = [];
  for (const layers of args.layers.split(","))
    for (const renderer of args.renderers.split(","))
      results.push(
        await run(browser, viewer.url, project.projectId, project.revision, renderer, layers),
      );
  const summary = { nodes: project.nodes, frames: 12, dpr: 2, viewport: "1440x900", results };
  await writeFile(join(OUT, "summary.json"), JSON.stringify(summary, null, 2) + "\n");
  console.log(JSON.stringify(summary, null, 1));
  // The canvas path: with the Layers panel closed nothing else renders per node.
  const vello = results.find((r) => r.renderer === "vello" && r.layers === "closed");
  if (args.strict && vello && vello.longTasks.all.maxMs > 50)
    throw new Error(`vello main-thread long task ${vello.longTasks.all.maxMs} ms`);
} finally {
  await browser?.close();
  await viewer?.stop();
  await project?.close();
  await backend.stop();
  await rm(join(OUT, "dist"), { recursive: true, force: true });
}
