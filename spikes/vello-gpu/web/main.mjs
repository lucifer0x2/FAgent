// Task 29 demo: scene-3 drawn by vello_gpu through WebGPU (default) or WebGL2.
// Query: ?renderer=webgpu|webgl2 (WebGPU falls back to WebGL2 when its init fails),
// ?scene=<url of a scene JSON> (default /out/board/board.json, "none" = harness only),
// ?pkg=speed|size (wasm build profile), ?shadows=filter|analytic, ?cull=off (no viewport culling),
// ?text=outlines|glyph_run|atlas, ?greek=<device px> (text LOD, default 0 = off).
// Drag to pan, wheel to zoom (around the pointer), "f" to fit. Harnesses use window.vello.
const params = new URLSearchParams(location.search);
const pkg = params.get("pkg") ?? "speed";
const status = document.getElementById("status");

const wasmStart = performance.now();
const bindings = await import(`../pkg/${pkg}/vello_demo.js`);
await bindings.default();
const wasmMs = performance.now() - wasmStart;

/** Yields to the event loop without rAF throttling (lets WebGPU callbacks run). */
const yieldTask = () => new Promise(resolve => {
  const channel = new MessageChannel();
  channel.port1.onmessage = () => resolve();
  channel.port2.postMessage(null);
});

async function decodeImage(blob) {
  const bitmap = await createImageBitmap(blob, { premultiplyAlpha: "none", colorSpaceConversion: "none" });
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  ctx.drawImage(bitmap, 0, 0);
  const data = ctx.getImageData(0, 0, bitmap.width, bitmap.height).data;
  bitmap.close();
  return { data: new Uint8Array(data.buffer), width: canvas.width, height: canvas.height };
}

/** One renderer bound to one canvas (a canvas keeps its first context type). */
class Viewer {
  static async create(canvas, renderer = "webgpu", { fallback = true } = {}) {
    let demo, fallbackReason = null;
    if (renderer === "webgpu") {
      try {
        if (!navigator.gpu) throw Error("navigator.gpu missing");
        demo = await bindings.Demo.webgpu(canvas);
      } catch (error) {
        if (!fallback) throw error;
        fallbackReason = String(error?.message ?? error);
        // A failed WebGPU attempt may have claimed the canvas: use a fresh one.
        const fresh = canvas.cloneNode();
        canvas.replaceWith(fresh);
        canvas = fresh;
      }
    }
    demo ??= bindings.Demo.webgl2(canvas);
    return new Viewer(demo, canvas, fallbackReason);
  }

  constructor(demo, canvas, fallbackReason) {
    this.demo = demo;
    this.canvas = canvas;
    this.backend = demo.backend();
    this.initMs = demo.init_ms;
    this.fallbackReason = fallbackReason;
    this.camera = { scale: 1, tx: 0, ty: 0 };
    this.analytic = params.get("shadows") === "analytic";
    this.cull = params.get("cull") !== "off";
    this.setText(params.get("text") ?? "outlines");
    this.setGreek(Number(params.get("greek") ?? 0));
    this.loaded = new Set();
  }

  /** "outlines" (default; same path as vello_cpu), "glyph_run" or "atlas" (glyph atlas). */
  setText(mode) {
    this.demo.set_text_mode(mode);
    this.text = mode;
  }

  /** Level of detail: text under `px` device px becomes one bar per line (0 = off). */
  setGreek(px) {
    this.demo.set_greek_below(px);
    this.greek = px;
  }

  /** Fonts and images not loaded yet; URLs resolve against `base` (the scene URL). */
  async loadResources(scene, base) {
    const t0 = performance.now();
    const failed = [];
    for (const font of scene.fonts ?? []) {
      if (this.loaded.has(`font:${font.id}`)) continue;
      try {
        const bytes = new Uint8Array(await (await fetch(new URL(`.${font.url}`, base))).arrayBuffer());
        this.demo.add_font(JSON.stringify(font), bytes);
        this.loaded.add(`font:${font.id}`);
      } catch (error) { failed.push({ id: font.id, message: String(error?.message ?? error) }); }
    }
    const t1 = performance.now();
    for (const asset of scene.assets ?? []) {
      if (this.loaded.has(`image:${asset.id}`)) continue;
      try {
        const image = await decodeImage(await (await fetch(new URL(`.${asset.url}`, base))).blob());
        this.demo.add_image(asset.id, image.data, image.width, image.height);
        this.loaded.add(`image:${asset.id}`);
      } catch (error) { failed.push({ id: asset.id, message: String(error?.message ?? error) }); }
    }
    return { fontMs: t1 - t0, imageMs: performance.now() - t1, failed };
  }

  setScene(scene) {
    const t0 = performance.now();
    const json = typeof scene === "string" ? scene : JSON.stringify(scene);
    this.viewport = JSON.parse(json).provenance?.viewport ?? { width: 1, height: 1 };
    const nodes = this.demo.set_scene(json);
    return { nodes, parseMs: performance.now() - t0 };
  }

  /** Matches the canvas backing store to its CSS size × DPR (or an explicit size). */
  resize(width = Math.round(this.canvas.clientWidth * devicePixelRatio),
    height = Math.round(this.canvas.clientHeight * devicePixelRatio)) {
    if (this.canvas.width !== width || this.canvas.height !== height) {
      this.canvas.width = width;
      this.canvas.height = height;
    }
    this.demo.resize();
  }

  /** Whole scene viewport in the canvas with a 24 CSS px margin. */
  fit() {
    const margin = 24 * devicePixelRatio;
    const { width, height } = this.viewport;
    const scale = Math.min((this.canvas.width - 2 * margin) / width, (this.canvas.height - 2 * margin) / height);
    this.camera = { scale, tx: (this.canvas.width - width * scale) / 2, ty: (this.canvas.height - height * scale) / 2 };
    return this.camera;
  }

  render(camera = this.camera, analytic = this.analytic, cull = this.cull) {
    const [encodeMs, submitMs, nodes, glyphs, layers, culled, greeked] =
      this.demo.render(camera.scale, camera.tx, camera.ty, analytic, cull);
    return { encodeMs, submitMs, nodes, glyphs, layers, culled, greeked };
  }

  /** Waits until the GPU finished the last frame; WebGL2 blocks in gl.finish(). */
  async gpuDone(limitMs = 10_000) {
    const t0 = performance.now();
    while (!this.demo.gpu_idle()) {
      if (performance.now() - t0 > limitMs) throw Error("GPU did not finish");
      await yieldTask();
    }
    return performance.now() - t0;
  }

  /** Render + wait for the GPU: the time a full redraw costs. */
  async timedFrame(camera, analytic, cull) {
    const t0 = performance.now();
    const r = this.render(camera, analytic, cull);
    const waitMs = await this.gpuDone();
    return { ...r, waitMs, gpuCompleteMs: performance.now() - t0 };
  }
}

const vello = { Viewer, wasmMs, pkg, yieldTask, viewer: null };
window.vello = vello;

function interactive(viewer) {
  const view = viewer.canvas;
  let dirty = true, drag = null, last = {};
  const request = () => { dirty = true; };
  const loop = () => {
    if (dirty) {
      dirty = false;
      last = viewer.render();
      const { scale } = viewer.camera;
      status.textContent = `${viewer.backend}${viewer.fallbackReason ? ` (fallback: ${viewer.fallbackReason})` : ""}` +
        `  zoom ${(scale / devicePixelRatio * 100).toFixed(1)}%  encode ${last.encodeMs.toFixed(1)} ms` +
        `  submit ${last.submitMs.toFixed(1)} ms  nodes ${last.nodes}  layers ${last.layers}`;
    }
    requestAnimationFrame(loop);
  };
  view.addEventListener("pointerdown", e => { drag = { x: e.clientX, y: e.clientY }; view.setPointerCapture(e.pointerId); });
  view.addEventListener("pointerup", () => { drag = null; });
  view.addEventListener("pointermove", e => {
    if (!drag) return;
    viewer.camera.tx += (e.clientX - drag.x) * devicePixelRatio;
    viewer.camera.ty += (e.clientY - drag.y) * devicePixelRatio;
    drag = { x: e.clientX, y: e.clientY };
    request();
  });
  view.addEventListener("wheel", e => {
    e.preventDefault();
    const c = viewer.camera, k = Math.exp(-e.deltaY * 0.002);
    const px = e.offsetX * devicePixelRatio, py = e.offsetY * devicePixelRatio;
    viewer.camera = { scale: c.scale * k, tx: px - (px - c.tx) * k, ty: py - (py - c.ty) * k };
    request();
  }, { passive: false });
  addEventListener("keydown", e => { if (e.key === "f") { viewer.fit(); request(); } });
  addEventListener("resize", () => { viewer.resize(); request(); });
  vello.redraw = request;
  requestAnimationFrame(loop);
}

const sceneUrl = params.get("scene") ?? "/out/board/board.json";
try {
  if (sceneUrl !== "none") {
    const viewer = await Viewer.create(document.getElementById("view"), params.get("renderer") ?? "webgpu");
    vello.viewer = viewer;
    const base = new URL(sceneUrl, location.href);
    const json = await (await fetch(base)).text();
    const set = viewer.setScene(json);
    const res = await viewer.loadResources(JSON.parse(json), base);
    viewer.resize();
    viewer.fit();
    // The first frame includes pipeline creation / shader compile not covered by init.
    const first = await viewer.timedFrame();
    vello.boot = { backend: viewer.backend, fallbackReason: viewer.fallbackReason, wasmMs, initMs: viewer.initMs,
      firstFrameMs: first.gpuCompleteMs, ...set, ...res };
    interactive(viewer);
  } else {
    status.textContent = `harness mode (wasm ${wasmMs.toFixed(0)} ms)`;
  }
  vello.ready = true;
} catch (error) {
  vello.error = String(error?.stack ?? error);
  status.textContent = `error: ${error?.message ?? error}`;
}
