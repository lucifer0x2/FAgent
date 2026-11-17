// vello_gpu captures of the parity cases in the pinned Chromium (real GPU, see
// browser.mjs): one canvas per backend (WebGPU, WebGL2; no fallback), resized per
// case to ceil(viewport × scale). Pixels are read with canvas.toDataURL in the same
// task as the render, before the frame is presented. Text uses the outline path
// shared with vello_cpu unless `text` is "atlas"; shadows use the filter layer unless `analytic`.
import { relative } from "node:path";
import { withDemo } from "./browser.mjs";

/** Returns {webgpu, webgl2}: Map "name@scale" → {ok, png (base64)} | {ok:false, message}. */
export async function gpuCaptures(root, out, cases, { analytic = false, text = "outlines" } = {}) {
  const base = `/${relative(`${root}/spikes/vello-gpu`, out)}/`;
  const jobs = cases.flatMap(c => c.scales.map(scale => ({ key: `${c.name}@${scale}`, scene: c.scene, scale })));
  const fonts = new Map(), assets = new Map();
  for (const c of cases) {
    for (const f of c.scene.fonts ?? []) fonts.set(f.id, f);
    for (const a of c.scene.assets ?? []) assets.set(a.id, a);
  }
  const resources = { fonts: [...fonts.values()], assets: [...assets.values()] };
  const result = {};
  await withDemo(root, { width: 800, height: 600, dpr: 1 }, async ({ page, gpu, errors }) => {
    console.error(`gpu-capture: ${JSON.stringify(gpu)}`);
    for (const backend of ["webgpu", "webgl2"]) {
      const shots = await page.evaluate(async ({ backend, base, resources, jobs, analytic, text }) => {
        const canvas = document.createElement("canvas");
        canvas.style.cssText = "position:fixed;left:0;top:0";
        document.body.append(canvas);
        const viewer = await window.vello.Viewer.create(canvas, backend, { fallback: false });
        viewer.setText(text);
        const loaded = await viewer.loadResources(resources, new URL(base, location.href));
        const shots = {};
        for (const { key, scene, scale } of jobs) {
          try {
            const { width, height } = scene.provenance.viewport;
            viewer.resize(Math.ceil(width * scale), Math.ceil(height * scale));
            viewer.setScene(scene);
            viewer.render({ scale, tx: 0, ty: 0 }, analytic, true);
            shots[key] = { ok: true, png: viewer.canvas.toDataURL("image/png").split(",")[1] };
            await viewer.gpuDone();
          } catch (error) {
            shots[key] = { ok: false, message: String(error?.message ?? error) };
          }
        }
        canvas.remove();
        return { shots, failed: loaded.failed, backend: viewer.backend };
      }, { backend, base, resources, jobs, analytic, text });
      if (shots.backend !== backend) throw Error(`${backend}: got ${shots.backend}`);
      if (shots.failed.length) console.error(`gpu-capture ${backend}: resources failed ${JSON.stringify(shots.failed)}`);
      result[backend] = new Map(Object.entries(shots.shots));
    }
    if (errors.length) console.error(`gpu-capture page errors: ${errors.slice(0, 5).join(" | ")}`);
  });
  return result;
}
