// Worker side: the same engine-wasm module and API the viewer Worker uses
// (web/src/engine/engine.worker.ts), fed precomputed scenes with insert_scene.
let engine, canvas;

async function rgba(blob) {
  const bitmap = await createImageBitmap(blob, { premultiplyAlpha: "none", colorSpaceConversion: "none" });
  const c = new OffscreenCanvas(bitmap.width, bitmap.height);
  const ctx = c.getContext("2d", { willReadFrequently: true });
  ctx.drawImage(bitmap, 0, 0);
  const { data } = ctx.getImageData(0, 0, bitmap.width, bitmap.height);
  return { data: new Uint8Array(data.buffer), width: bitmap.width, height: bitmap.height };
}

const base64 = (buffer) => {
  let text = "";
  const bytes = new Uint8Array(buffer);
  for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(text);
};

async function init({ canvas: target, backend, resources, base, analytic }) {
  const manifest = await (await fetch("../pkg/engine/manifest.json", { cache: "no-store" })).json();
  const module = await import(new URL(`../pkg/engine/${manifest.js}`, import.meta.url).href);
  await module.default({ module_or_path: new URL(`../pkg/engine/${manifest.wasm}`, import.meta.url).href });
  engine = new module.Engine();
  canvas = target;
  const surface =
    backend === "webgpu" ? await module.Surface.webgpu(canvas)
    : backend === "webgl2" ? module.Surface.webgl2(canvas)
    : module.Surface.cpu(canvas);
  engine.attach(surface);
  engine.start_probe();
  for (;;) {
    const status = engine.probe_status();
    if (status === "ok") break;
    if (status !== "pending") throw new Error(status);
    await new Promise((r) => setTimeout(r, 0));
  }
  // Exact scene captures: no LOD at parity scales, no viewer backdrop.
  engine.set_draw_options(0, analytic ? 1e9 : 0, false);
  const failed = [];
  for (const font of resources.fonts) {
    try {
      const bytes = new Uint8Array(await (await fetch(new URL(`.${font.url}`, base))).arrayBuffer());
      engine.load_font(font.id, font.hash, bytes);
    } catch (error) { failed.push({ id: font.id, message: String(error?.message ?? error) }); }
  }
  for (const asset of resources.assets) {
    try {
      const image = await rgba(await (await fetch(new URL(`.${asset.url}`, base))).blob());
      engine.add_image(asset.id, asset.hash, image.data, image.width, image.height);
    } catch (error) { failed.push({ id: asset.id, message: String(error?.message ?? error) }); }
  }
  return { backend: engine.backend(), failed };
}

async function shot({ scene, scale }) {
  const { frameId, viewport } = scene.provenance;
  engine.insert_scene(JSON.stringify(scene));
  engine.commit(frameId);
  engine.set_layout(JSON.stringify([{ id: frameId, x: 0, y: 0, width: viewport.width, height: viewport.height }]));
  engine.set_camera(scale, 0, 0, Math.ceil(viewport.width * scale), Math.ceil(viewport.height * scale));
  engine.render();
  // Same task as the render: the WebGPU canvas texture is still current.
  const blob = await canvas.convertToBlob({ type: "image/png" });
  while (!engine.gpu_idle()) await new Promise((r) => setTimeout(r, 0));
  return { ok: true, png: base64(await blob.arrayBuffer()) };
}

self.onmessage = async ({ data }) => {
  try {
    const value = data.type === "init" ? await init(data) : await shot(data);
    self.postMessage({ id: data.id, ...value });
  } catch (error) {
    self.postMessage({ id: data.id, ok: false, error: String(error?.message ?? error), message: String(error?.message ?? error) });
  }
};
