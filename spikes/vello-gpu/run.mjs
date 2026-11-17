// Task 29 measurements on the exported board (export-board.mjs) in the pinned
// Chromium (full build, new headless, real GPU), 1440×900 CSS px at DPR 2, plus
// vello_cpu natively through the CLI. Per backend (WebGPU, WebGL2) and variant:
// encode / submit / GPU-complete ms at Fit and 100%, pan frame times over rAF frames,
// device + shader init. Usage: node spikes/vello-gpu/run.mjs [--frames 20] [--pan 150]
//   (needs node spikes/vello-gpu/build.mjs and export-board.mjs first)
import { execFile } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { withDemo } from "./browser.mjs";

const exec = promisify(execFile);
const here = dirname(fileURLToPath(import.meta.url)), root = resolve(here, "../..");
const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? Number(process.argv[i + 1]) : fallback;
};
const FRAMES = arg("frames", 20), PAN = arg("pan", 150), WIDTH = 1440, HEIGHT = 900, DPR = 2;
const out = join(here, "out", `run-${new Date().toISOString().replaceAll(":", "-")}`);
const board = JSON.parse(await readFile(join(here, "out/board/board.json"), "utf8"));
const sizes = JSON.parse(await readFile(join(here, "pkg/sizes.json"), "utf8"));

// Same camera as the demo's fit(): whole board, 24 CSS px margin. 100% = scale DPR at frame 2.
const [cw, ch] = [WIDTH * DPR, HEIGHT * DPR], margin = 24 * DPR, vp = board.provenance.viewport;
const fitScale = Math.min((cw - 2 * margin) / vp.width, (ch - 2 * margin) / vp.height);
const cameras = {
  fit: { scale: fitScale, tx: (cw - vp.width * fitScale) / 2, ty: (ch - vp.height * fitScale) / 2 },
  "100%": { scale: DPR, tx: -1640 * DPR, ty: -600 * DPR },
};
// text: outlines | atlas; greek: LOD bars below N device px; shadows: filter | analytic.
const variants = {
  baseline: { text: "outlines", greek: 0, analytic: false },
  analytic: { text: "outlines", greek: 0, analytic: true },
  atlas: { text: "atlas", greek: 0, analytic: false },
  "atlas+lod": { text: "atlas", greek: 4, analytic: false },
  "atlas+lod+analytic": { text: "atlas", greek: 4, analytic: true },
};
const pctl = (values, p) => {
  const s = [...values].sort((a, b) => a - b);
  return s.length ? s[Math.min(s.length - 1, Math.floor(p * s.length))] : null;
};
const stats = values => ({ p50: pctl(values, 0.5), p95: pctl(values, 0.95), max: Math.max(...values) });

async function measureBackend(renderer, pkg = "speed") {
  return withDemo(root, { width: WIDTH, height: HEIGHT, dpr: DPR, query: `renderer=${renderer}&pkg=${pkg}` },
    async ({ page, gpu, browserVersion, errors }) => {
      const boot = await page.evaluate(() => window.vello.boot);
      const result = { boot, gpu, browserVersion, frames: {}, pan: {} };
      for (const [name, v] of Object.entries(variants)) for (const [cam, camera] of Object.entries(cameras)) {
        result.frames[`${name}@${cam}`] = await page.evaluate(async ({ v, camera, n }) => {
          const viewer = window.vello.viewer;
          viewer.setText(v.text); viewer.setGreek(v.greek);
          const runs = [];
          for (let i = 0; i < n + 3; i++) runs.push(await viewer.timedFrame(camera, v.analytic, true));
          return runs.slice(3);
        }, { v, camera, n: FRAMES });
      }
      result.frames["baseline-nocull@100%"] = await page.evaluate(async ({ camera, n }) => {
        const viewer = window.vello.viewer; viewer.setText("outlines"); viewer.setGreek(0);
        const runs = [];
        for (let i = 0; i < n + 3; i++) runs.push(await viewer.timedFrame(camera, false, false));
        return runs.slice(3);
      }, { camera: cameras["100%"], n: FRAMES });
      for (const name of ["baseline", "atlas+lod+analytic"]) for (const [cam, camera] of Object.entries(cameras)) {
        const v = variants[name];
        // rAF-driven pan: 8 device px per frame, one full redraw per frame.
        result.pan[`${name}@${cam}`] = await page.evaluate(({ v, camera, frames }) => new Promise(done => {
          const viewer = window.vello.viewer;
          viewer.setText(v.text); viewer.setGreek(v.greek);
          const stamps = [], work = [];
          let i = 0;
          const step = now => {
            stamps.push(now);
            const t0 = performance.now();
            viewer.render({ ...camera, tx: camera.tx - 8 * i }, v.analytic, true);
            work.push(performance.now() - t0);
            if (++i <= frames) requestAnimationFrame(step); else done({ stamps, work });
          };
          requestAnimationFrame(step);
        }), { v, camera, frames: PAN });
      }
      result.errors = errors;
      return result;
    });
}

async function cpuNative() {
  await exec("cargo", ["build", "--release", "--locked", "-p", "fagent-paint-vello", "--features", "cli"],
    { cwd: root, maxBuffer: 16 << 20 });
  const binary = process.env.PAINT_VELLO_BIN ?? join(root, "target/release/paint-vello-png");
  const results = {};
  for (const [name, v] of Object.entries({ baseline: variants.baseline, "lod+analytic": { greek: 4, analytic: true } }))
    for (const [cam, camera] of Object.entries(cameras)) {
      const request = join(out, `cpu-${name}-${cam.replace("%", "")}.json`);
      await writeFile(request, JSON.stringify({
        scene: board, scale: camera.scale, size: [cw, ch], translate: [camera.tx, camera.ty],
        shadows: v.analytic ? "analytic" : "filter", greekBelowPx: v.greek, repeat: 6,
      }));
      const r = JSON.parse((await exec(binary, [join(here, "out/board"), request, join(out, `cpu-${name}-${cam.replace("%", "")}.png`)],
        { maxBuffer: 16 << 20 })).stdout);
      if (!r.ok) throw Error(JSON.stringify(r));
      const runs = r.runs.slice(1);
      results[`${name}@${cam}`] = { encode: stats(runs.map(x => x.encode)), total: stats(runs.map(x => x.total)), stats: r.stats };
    }
  return results;
}

await mkdir(out, { recursive: true });
const report = { schema: "vello-run/1", viewport: { width: WIDTH, height: HEIGHT, dpr: DPR }, cameras,
  board: { nodes: board.nodes.length, viewport: vp }, wasm: sizes, backends: {} };
for (const renderer of ["webgpu", "webgl2"]) report.backends[renderer] = await measureBackend(renderer);
report.scalarWasm = (await measureBackend("webgpu", "scalar").catch(e => ({ error: String(e) })));
report.cpu = await cpuNative();
await writeFile(join(out, "raw.json"), JSON.stringify(report) + "\n");

// Summary: p50/p95 of each frame metric, pan frame interval/work percentiles.
const summarize = r => {
  const frames = Object.fromEntries(Object.entries(r.frames ?? {}).map(([k, runs]) => [k, {
    encode: stats(runs.map(x => x.encodeMs)), submit: stats(runs.map(x => x.submitMs)),
    gpuComplete: stats(runs.map(x => x.gpuCompleteMs)), culled: runs[0].culled, greeked: runs[0].greeked,
    glyphs: runs[0].glyphs, layers: runs[0].layers,
  }]));
  const pan = Object.fromEntries(Object.entries(r.pan ?? {}).map(([k, { stamps, work }]) => {
    const intervals = stamps.slice(2).map((t, i) => t - stamps[i + 1]);
    return [k, { frames: intervals.length, interval: stats(intervals), work: stats(work.slice(1)),
      over20ms: intervals.filter(x => x > 20).length }];
  }));
  return { boot: r.boot, gpu: r.gpu, frames, pan, errors: r.errors };
};
const summary = { ...report, backends: Object.fromEntries(Object.entries(report.backends).map(([k, r]) => [k, summarize(r)])),
  scalarWasm: report.scalarWasm.error ? report.scalarWasm : summarize({ ...report.scalarWasm, pan: {} }).frames["baseline@fit"] };
await writeFile(join(out, "summary.json"), JSON.stringify(summary, null, 2) + "\n");
const f = x => x === null || x === undefined ? "-" : x.toFixed(1);
console.log("| backend | variant@camera | encode p50 | submit p50 | GPU-complete p50 / p95 |\n| --- | --- | --- | --- | --- |");
for (const [b, s] of Object.entries(summary.backends)) for (const [k, v] of Object.entries(s.frames))
  console.log(`| ${b} | ${k} | ${f(v.encode.p50)} | ${f(v.submit.p50)} | ${f(v.gpuComplete.p50)} / ${f(v.gpuComplete.p95)} |`);
console.log("\n| backend | pan | interval p50 / p95 | work p50 / p95 | >20 ms |\n| --- | --- | --- | --- | --- |");
for (const [b, s] of Object.entries(summary.backends)) for (const [k, v] of Object.entries(s.pan))
  console.log(`| ${b} | ${k} | ${f(v.interval.p50)} / ${f(v.interval.p95)} | ${f(v.work.p50)} / ${f(v.work.p95)} | ${v.over20ms}/${v.frames} |`);
console.log("\n| vello_cpu native | encode p50 | encode+raster p50 |\n| --- | --- | --- |");
for (const [k, v] of Object.entries(summary.cpu)) console.log(`| ${k} | ${f(v.encode.p50)} | ${f(v.total.p50)} |`);
for (const [b, s] of Object.entries(summary.backends))
  console.log(`${b}: wasm ${f(s.boot.wasmMs)} ms, init ${f(s.boot.initMs)} ms, first frame ${f(s.boot.firstFrameMs)} ms, gpu ${JSON.stringify(s.gpu)}`);
console.log(JSON.stringify({ summary: relative(root, join(out, "summary.json")), scalarWasmBaselineFit: summary.scalarWasm }));
