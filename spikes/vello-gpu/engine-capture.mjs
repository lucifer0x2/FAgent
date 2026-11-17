// Task 30 parity: the viewer engine (crates/engine-wasm, the module renderer=vello loads)
// captured in a Worker on an OffscreenCanvas for WebGPU, WebGL2 and vello_cpu, on the same
// cases as gpu-capture.mjs. Copies web/public/engine-wasm (npm run build:engine-wasm) to
// pkg/engine/ so the loopback server can serve it next to engine/.
import { cp, rm } from "node:fs/promises";
import { join, relative } from "node:path";
import { serve } from "./serve.mjs";

/** Returns {"engine-webgpu", "engine-webgl2", "engine-cpu"}: Map "name@scale" → {ok, png}. */
export async function engineCaptures(root, out, cases, { analytic = false } = {}) {
  const here = join(root, "spikes/vello-gpu");
  await rm(join(here, "pkg/engine"), { recursive: true, force: true });
  await cp(join(root, "web/public/engine-wasm"), join(here, "pkg/engine"), { recursive: true });
  const base = `/${relative(here, out)}/`;
  const jobs = cases.flatMap((c) =>
    c.scales.map((scale) => ({ key: `${c.name}@${scale}`, scene: c.scene, scale })),
  );
  const fonts = new Map(),
    assets = new Map();
  for (const c of cases) {
    for (const f of c.scene.fonts ?? []) fonts.set(f.id, f);
    for (const a of c.scene.assets ?? []) assets.set(a.id, a);
  }
  const resources = { fonts: [...fonts.values()], assets: [...assets.values()] };
  process.env.PLAYWRIGHT_BROWSERS_PATH = join(root, ".runtime/browsers");
  const { chromium } = await import("playwright");
  const server = await serve();
  const browser = await chromium.launch({
    channel: "chromium",
    headless: true,
    args: ["--force-color-profile=srgb"],
  });
  const result = {};
  try {
    const page = await browser.newPage({ viewport: { width: 800, height: 600 } });
    const errors = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    await page.goto(`${server.origin}/engine/`);
    await page.waitForFunction(() => window.engineCaptureReady);
    for (const backend of ["webgpu", "webgl2", "cpu"]) {
      const shots = await page.evaluate(
        (input) => window.engineCapture(input),
        { backend, resources, base: new URL(base, server.origin).href, jobs, analytic },
      );
      if (shots.backend !== backend)
        throw Error(`engine ${backend}: got ${shots.backend} (${shots.error ?? ""})`);
      if (shots.failed?.length)
        console.error(`engine-capture ${backend}: resources failed ${JSON.stringify(shots.failed)}`);
      result[`engine-${backend}`] = new Map(Object.entries(shots.shots));
    }
    if (errors.length) console.error(`engine-capture page errors: ${errors.slice(0, 5).join(" | ")}`);
  } finally {
    await browser.close();
    await server.close();
  }
  return result;
}
