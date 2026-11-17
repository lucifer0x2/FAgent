// Task 29 parity (Task 34: the cpu column is the server's vello export): Chromium Canvas2D (renderer/) vs vello_cpu on the
// native-paint-parity fixtures, with that harness's straight-RGBA gate
// (a pixel fails when any channel differs by > 8; a case passes below 0.5%).
// Usage: node spikes/vello-gpu/parity.mjs [--gpu] [--engine] [--analytic] [--atlas]
//   --engine adds the Task 30 viewer engine (engine-wasm in a Worker on OffscreenCanvas)
//   for WebGPU/WebGL2/vello_cpu, with its difference from the Task 29 column (vsP0).
//   --atlas: GPU text through the vello_gpu glyph atlas (vello_cpu keeps outlines).
//   --gpu adds vello_gpu WebGPU/WebGL2 columns; --analytic draws shadows with Vello's
//   analytic blurred rounded rect instead of the default filter layer.
import { mkdir, writeFile, readFile } from "node:fs/promises";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";
import { dirname, join, relative, resolve } from "node:path";
import { fixtures } from "../native-paint-parity/fixtures.mjs";
import { withOracle, capture } from "../native-paint-parity/oracle.mjs";
import { decodePNG, encodePNG } from "../native-paint-parity/png.mjs";
import { compare } from "../native-paint-parity/compare.mjs";
import { groupOf } from "../native-paint-parity/groups.mjs";
import { prepareResources, withoutMissingFonts } from "./resources.mjs";
import { gpuCaptures } from "./gpu-capture.mjs";
import { engineCaptures } from "./engine-capture.mjs";

const exec = promisify(execFile);
const analytic = process.argv.includes("--analytic");
const here = dirname(fileURLToPath(import.meta.url)), root = resolve(here, "../..");
const out = join(here, "out", `parity-${new Date().toISOString().replaceAll(":", "-")}`);
await mkdir(out, { recursive: true });
// The cpu column is the server's strict `vello` export (paint-vello-export: filter shadows, outline
// glyphs, shared image decode and PNG encode); --analytic needs the P0 CLI, which can switch shadows.
const cli = analytic ? "paint-vello-png" : "paint-vello-export";
const binary = process.env.PAINT_VELLO_BIN ?? join(root, "target/release", cli);
if (!process.env.PAINT_VELLO_BIN) {
  await exec("cargo", ["build", "--release", "--locked", "--offline", "-p", "fagent-paint-vello", "--features", "cli", "--bin", cli],
    { cwd: root, maxBuffer: 16 << 20 });
}

const rows = [];
// Gate metric is straight RGBA (as native-paint-parity); `white` is the same over white
// (premultiplied-equivalent: what a viewer shows), `vsCpu` compares a GPU backend to vello_cpu.
const verdictOf = (category, a, b, dir, label, cpuPng) => {
  const c = compare(decodePNG(a), decodePNG(b), { category });
  const vsCpu = cpuPng ? compare(decodePNG(cpuPng), decodePNG(b), { category }).report.visibleStraight : null;
  return writeFile(join(dir, `chrome-${label}-diff.png`), encodePNG(c.sideBySide)).then(() => ({
    verdict: c.report.verdict,
    fractionOver8: c.report.visibleStraight.fractionOver8,
    maxDelta: c.report.visibleStraight.maxDelta,
    whiteOver8: c.report.compositedWhite.fractionOver8,
    whiteMax: c.report.compositedWhite.maxDelta,
    ...(vsCpu && { vsCpuOver8: vsCpu.fractionOver8, vsCpuMax: vsCpu.maxDelta }),
  }));
};

await withOracle(root, out, async oracle => {
  const resources = await prepareResources(oracle.page, root, out);
  await oracle.refreshResources();
  const cases = withoutMissingFonts(fixtures(resources), resources);
  const gpu = process.argv.includes("--gpu") ? await gpuCaptures(root, out, cases, { analytic, text: process.argv.includes("--atlas") ? "atlas" : "outlines" }) : null;
  const engine = process.argv.includes("--engine") ? await engineCaptures(root, out, cases, { analytic }) : null;
  for (const fixture of cases) for (const scale of fixture.scales) {
    const key = `${fixture.name}@${scale}`, dir = join(out, key);
    await mkdir(dir);
    const request = join(dir, "request.json");
    await writeFile(request, JSON.stringify({ scene: fixture.scene, scale, shadows: analytic ? "analytic" : "filter" }));
    const row = { name: fixture.name, category: fixture.category, scale };
    rows.push(row);
    const chrome = await capture(oracle.page, fixture.scene, scale);
    if (!chrome.ok) { row.error = `chrome ${chrome.code}`; continue; }
    const chromePng = Buffer.from(chrome.png, "base64");
    await writeFile(join(dir, "chrome.png"), chromePng);
    const cpu = JSON.parse((await exec(binary, [out, request, join(dir, "vello-cpu.png")])).stdout);
    if (!cpu.ok) { row.cpu = { verdict: "ERROR", code: cpu.code, message: cpu.message }; continue; }
    const cpuPng = await readFile(join(dir, "vello-cpu.png"));
    row.cpu = await verdictOf(fixture.category, chromePng, cpuPng, dir, "cpu");
    const shown = { cpu: cpuPng };
    for (const [backend, shots] of Object.entries({ ...gpu, ...engine })) {
      const png = shots.get(key);
      if (!png?.ok) { row[backend] = { verdict: "ERROR", message: png?.message ?? "missing" }; continue; }
      const bytes = Buffer.from(png.png, "base64");
      shown[backend] = bytes;
      await writeFile(join(dir, `${backend}.png`), bytes);
      row[backend] = await verdictOf(fixture.category, chromePng, bytes, dir, backend, cpuPng);
      // Engine (Task 30) against the Task 29 capture of the same backend.
      const p0 = shown[backend.replace(/^engine-/, "")];
      if (backend.startsWith("engine-") && p0) {
        const d = compare(decodePNG(p0), decodePNG(bytes), { category: fixture.category }).report.visibleStraight;
        row[backend].vsP0Over8 = d.fractionOver8;
        row[backend].vsP0Max = d.maxDelta;
      }
    }
  }
});

const backends = ["cpu", "webgpu", "webgl2", "engine-webgpu", "engine-webgl2", "engine-cpu"].filter(b => rows.some(r => r[b]));
const summary = Object.fromEntries(backends.map(b => [b, rows.reduce((acc, r) => {
  const v = r[b]?.verdict ?? "MISSING";
  acc[v] = (acc[v] ?? 0) + 1;
  return acc;
}, {})]));
await writeFile(join(out, "report.json"), JSON.stringify({ schema: "vello-parity/1", out: relative(root, out), summary, rows }, null, 2) + "\n");
const pct = v => v?.fractionOver8 === undefined ? (v?.verdict ?? "-") : `${v.verdict} ${(v.fractionOver8 * 100).toFixed(3)}%`;
console.log(`| case | ${backends.join(" | ")} |\n| --- |${backends.map(() => " --- |").join("")}`);
for (const r of rows) console.log(`| ${r.name}@${r.scale} | ${backends.map(b => pct(r[b])).join(" | ")} |`);
console.log(JSON.stringify({ report: relative(root, join(out, "report.json")), summary }));

// Feature groups: pass count, worst straight / over-white fraction > 8, worst GPU-vs-CPU fraction.
const groups = new Map();
for (const r of rows) {
  const g = groupOf(r.name);
  if (!groups.has(g)) groups.set(g, []);
  groups.get(g).push(r);
}
const worst = (rs, b, key) => Math.max(...rs.map(r => r[b]?.[key] ?? 0));
console.log(`\n| group | cases | ${backends.map(b => `${b} pass · max% · white max%`).join(" | ")} | GPU vs cpu max% |`);
console.log(`| --- | --- |${backends.map(() => " --- |").join("")} --- |`);
const groupRows = [];
for (const [g, rs] of groups) {
  const cells = backends.map(b => {
    const pass = rs.filter(r => r[b]?.verdict === "PASS").length;
    const text = rs.every(r => r[b]?.verdict === "REVIEW_TEXT");
    return `${text ? "review" : `${pass}/${rs.length}`} · ${(worst(rs, b, "fractionOver8") * 100).toFixed(2)} · ${(worst(rs, b, "whiteOver8") * 100).toFixed(2)}`;
  });
  const gpuVsCpu = Math.max(worst(rs, "webgpu", "vsCpuOver8"), worst(rs, "webgl2", "vsCpuOver8"));
  groupRows.push({ group: g, cases: rs.length, cells, gpuVsCpu });
  console.log(`| ${g} | ${rs.length} | ${cells.join(" | ")} | ${backends.length > 1 ? (gpuVsCpu * 100).toFixed(2) : "-"} |`);
}
await writeFile(join(out, "groups.json"), JSON.stringify(groupRows, null, 2) + "\n");
