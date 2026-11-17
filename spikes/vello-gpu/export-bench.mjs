// Task 34 export timing: the 12-frame / 4 944-node repaint board through real server jobs
// (MCP render_frame → ready PNG) for RENDER_BACKEND=chromium|vello. Every start uses a
// temporary SQLite root and port 0 (removed afterwards); nothing touches ~/.fagent or a live port.
//   cold  = first pass over the 12 frames right after the server starts (frame-0 includes
//           any worker/browser startup); scale exactly 1, PNG hashes recorded.
//   warm  = later passes; each uses scale 1 − k·1e-6 so the job key changes (no cache hit)
//           while the raster size stays the same.
// Peak RSS sums the server's whole process tree (Node worker and Chromium included).
// Usage: node spikes/vello-gpu/export-bench.mjs [--backends vello,chromium] [--starts 2] [--warm 2] [--breakdown]
//   Needs a release server (REPAINT_SERVER_BIN, default target/release/fagent-server) and, for
//   chromium, renderer/dist + release layout-step (FAGENT_LAYOUT_BIN) + .runtime/browsers.
//   → spikes/vello-gpu/out/bench-<time>/summary.json (ignored)
import { spawn, execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { HEIGHTS, seedProject } from "../../web/tests/repaint-fixture.mjs";
import { connectMcp, waitFor } from "../e2e/protocol.mjs";

const exec = promisify(execFile);
const ROOT = fileURLToPath(new URL("../../", import.meta.url));
const arg = (name, fallback) => {
  const i = process.argv.indexOf(name);
  return i > 0 ? process.argv[i + 1] : fallback;
};
const backends = arg("--backends", "vello,chromium").split(",");
const starts = Number(arg("--starts", "2")), warmPasses = Number(arg("--warm", "2"));
const out = join(ROOT, "spikes/vello-gpu/out", `bench-${new Date().toISOString().replaceAll(":", "-")}`);

async function start(backend) {
  const dataRoot = await mkdtemp(join(tmpdir(), "fagent-export-bench-"));
  const env = { PATH: process.env.PATH, HOME: process.env.HOME, TMPDIR: process.env.TMPDIR ?? "",
    DATABASE_PROVIDER: "sqlite", FAGENT_LAYOUT_BIN: process.env.FAGENT_LAYOUT_BIN ?? join(ROOT, "target/release/layout-step"),
    PLAYWRIGHT_BROWSERS_PATH: join(ROOT, ".runtime/browsers") };
  const binary = process.env.REPAINT_SERVER_BIN ?? join(ROOT, "target/release/fagent-server");
  const started = performance.now();
  const child = spawn(binary, ["--data-root", dataRoot, "--port", "0", "--workspace", ROOT, "--render-backend", backend],
    { cwd: ROOT, detached: true, stdio: ["ignore", "pipe", "pipe"], env });
  let output = "";
  child.stdout.on("data", b => (output += b));
  child.stderr.on("data", b => (output += b));
  const base = await waitFor(() => {
    if (child.exitCode !== null) throw Error(output);
    return output.match(/FAgent (http:\/\/127\.0\.0\.1:\d+)/)?.[1];
  }, "bench backend", 30_000);
  return { base, child, startupMs: performance.now() - started, async stop() {
    if (child.exitCode === null) { process.kill(-child.pid, "SIGTERM"); await new Promise(r => child.once("exit", r)); }
    await rm(dataRoot, { recursive: true, force: true });
  } };
}

// Peak resident set of `pid` and all its descendants, sampled every 100 ms.
function rssSampler(pid) {
  let peak = 0, stopped = false;
  const sample = async () => {
    const { stdout } = await exec("ps", ["-A", "-o", "pid=,ppid=,rss="]);
    const rows = stdout.trim().split("\n").map(l => l.trim().split(/\s+/).map(Number));
    const tree = new Set([pid]);
    for (let grown = true; grown;) {
      grown = false;
      for (const [p, pp] of rows) if (tree.has(pp) && !tree.has(p)) { tree.add(p); grown = true; }
    }
    peak = Math.max(peak, rows.filter(([p]) => tree.has(p)).reduce((s, [, , rss]) => s + rss, 0));
  };
  const loop = (async () => { while (!stopped) { await sample().catch(() => {}); await new Promise(r => setTimeout(r, 100)); } })();
  return { async stop() { stopped = true; await loop; return peak * 1024; } };
}

async function render(mcp, base, project, i, scale) {
  const t0 = performance.now();
  let job = await mcp.call("render_frame", { projectId: project.projectId, revision: project.revision,
    frameId: `frame-${i}`, viewport: { width: 1440, height: HEIGHTS[i] }, scale });
  while (job.status === "pending") job = await mcp.call("render_frame", { projectId: project.projectId, jobId: job.jobId, waitMs: 10_000 });
  const ms = performance.now() - t0;
  if (job.status !== "ready") throw Error(`frame-${i}@${scale}: ${JSON.stringify(job.error)}`);
  const png = Buffer.from(await (await fetch(new URL(job.imageUrl, base))).arrayBuffer());
  return { ms, jobId: job.jobId, sha256: createHash("sha256").update(png).digest("hex"), engine: job.image?.provenance?.engineVersion ?? job.provenance?.engineVersion };
}

// --breakdown: the tallest frame's computed scene re-rendered 5× by the export adapter
// (target/release/paint-vello-export) to split vello time into render (prepare+encode+raster)
// and PNG encoding; the server job total minus these is compute plus job overhead.
async function breakdown(base, jobId) {
  const { scene } = await (await fetch(new URL(`/api/v1/jobs/${jobId}`, base))).json();
  const dir = join(out, "breakdown");
  await mkdir(join(dir, "resources"), { recursive: true });
  let n = 0;
  const local = async entry => {
    const name = `r${n++}.bin`;
    await writeFile(join(dir, "resources", name), Buffer.from(await (await fetch(new URL(entry.url, base))).arrayBuffer()));
    return { ...entry, url: `/resources/${name}` };
  };
  const fonts = [], assets = [];
  for (const f of scene.fonts) fonts.push(await local(f));
  for (const a of scene.assets) assets.push(await local(a));
  await writeFile(join(dir, "request.json"), JSON.stringify({ scene: { ...scene, fonts, assets }, scale: 1 }));
  const runs = [];
  for (let k = 0; k < 5; k++) {
    const png = join(dir, `run-${k}.png`), t0 = performance.now();
    const result = JSON.parse((await exec(join(ROOT, "target/release/paint-vello-export"), [dir, join(dir, "request.json"), png])).stdout);
    if (!result.ok) throw Error(`breakdown: ${result.code}`);
    runs.push({ processMs: performance.now() - t0, ...result.ms, sha256: createHash("sha256").update(await readFile(png)).digest("hex") });
  }
  return { frame: scene.provenance.frameId, viewport: scene.provenance.viewport, nodes: scene.nodes.length, runs };
}

// Computed geometry of one job: node bounds and saved text lines (text, x, y, width).
async function geometry(base, jobId) {
  const { scene } = await (await fetch(new URL(`/api/v1/jobs/${jobId}`, base))).json();
  return Object.fromEntries(scene.nodes.map(n => [n.id, { b: n.bounds, l: n.lines.map(l => [l.text, l.x, l.y, l.width]) }]));
}

// Phase 3 compute check: every node's bounds and lines against the chromium scene.
function compareGeometry(expected, actual) {
  let nodes = 0, boundsDiffer = 0, linesDiffer = 0, maxDelta = 0;
  for (const [id, e] of Object.entries(expected)) {
    const a = actual[id];
    nodes++;
    if (!a) { boundsDiffer++; continue; }
    const d = ["x", "y", "width", "height"].map(k => Math.abs(e.b[k] - a.b[k]));
    maxDelta = Math.max(maxDelta, ...d);
    if (d.some(v => v > 0)) boundsDiffer++;
    if (JSON.stringify(e.l) !== JSON.stringify(a.l)) linesDiffer++;
  }
  return { nodes, boundsDiffer, linesDiffer, maxBoundsDelta: maxDelta, extraNodes: Object.keys(actual).length - Object.keys(expected).length };
}

const pct = (xs, p) => { const s = [...xs].sort((a, b) => a - b); return s[Math.min(s.length - 1, Math.ceil(p * s.length) - 1)]; };
const stats = xs => ({ n: xs.length, p50: pct(xs, 0.5), p95: pct(xs, 0.95), max: Math.max(...xs) });
const summary = { board: { frames: HEIGHTS.length, width: 1440, heights: HEIGHTS }, starts, warmPasses, backends: {} };
const scenes = {};
await mkdir(out, { recursive: true });
for (const backend of backends) {
  const runs = [];
  for (let s = 0; s < starts; s++) {
    const server = await start(backend);
    try {
      const project = await seedProject(server.base);
      const mcp = await connectMcp(server.base);
      const rss = rssSampler(server.child.pid);
      const cold = [];
      for (let i = 0; i < HEIGHTS.length; i++) cold.push(await render(mcp, server.base, project, i, 1));
      if (s === 0) scenes[backend] = await Promise.all(cold.map(c => geometry(server.base, c.jobId)));
      if (s === 0 && backend === "vello" && process.argv.includes("--breakdown")) summary.breakdown = await breakdown(server.base, cold[HEIGHTS.indexOf(Math.max(...HEIGHTS))].jobId);
      const warm = [];
      for (let k = 1; k <= warmPasses; k++) for (let i = 0; i < HEIGHTS.length; i++) warm.push(await render(mcp, server.base, project, i, 1 - k * 1e-6));
      runs.push({ startupMs: server.startupMs, cold, warm, peakRssBytes: await rss.stop() });
      await mcp.close(); await project.close();
    } finally { await server.stop(); }
    console.error(`${backend} start ${s + 1}/${starts} done`);
  }
  const hashes = runs.map(r => r.cold.map(c => c.sha256).join());
  summary.backends[backend] = {
    engine: runs[0].cold[0].engine,
    startupMs: stats(runs.map(r => r.startupMs)),
    coldFirstFrameMs: stats(runs.map(r => r.cold[0].ms)),
    coldFrameMs: stats(runs.flatMap(r => r.cold.map(c => c.ms))),
    coldBoardMs: stats(runs.map(r => r.cold.reduce((s, c) => s + c.ms, 0))),
    warmFrameMs: stats(runs.flatMap(r => r.warm.map(c => c.ms))),
    warmBoardMs: stats(runs.flatMap(r => Array.from({ length: warmPasses }, (_, k) =>
      r.warm.slice(k * HEIGHTS.length, (k + 1) * HEIGHTS.length).reduce((s, c) => s + c.ms, 0)))),
    peakRssMiB: stats(runs.map(r => r.peakRssBytes / 2 ** 20)),
    coldPngIdenticalAcrossStarts: hashes.every(h => h === hashes[0]),
    coldPngSha256: runs[0].cold.map(c => c.sha256),
  };
  await writeFile(join(out, "summary.json"), JSON.stringify(summary, null, 2) + "\n");
}
if (scenes.chromium) {
  summary.computeVsChromium = Object.fromEntries(Object.keys(scenes).filter(b => b !== "chromium").map(b => {
    const rows = scenes.chromium.map((expected, i) => compareGeometry(expected, scenes[b][i]));
    const sum = key => rows.reduce((s, r) => s + r[key], 0);
    return [b, { frames: rows.length, nodes: sum("nodes"), boundsDiffer: sum("boundsDiffer"), linesDiffer: sum("linesDiffer"),
      extraNodes: sum("extraNodes"), maxBoundsDelta: Math.max(...rows.map(r => r.maxBoundsDelta)) }];
  }));
  await writeFile(join(out, "summary.json"), JSON.stringify(summary, null, 2) + "\n");
}
console.log(JSON.stringify(summary, (k, v) => (typeof v === "number" ? Math.round(v * 10) / 10 : v), 2));
