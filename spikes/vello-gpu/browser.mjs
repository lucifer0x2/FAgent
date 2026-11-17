// Pinned Playwright Chromium for the Vello demo. The full Chromium build in new
// headless mode (channel "chromium") exposes the real GPU on macOS: WebGPU adapter
// "apple / metal-3" and WebGL2 "ANGLE Metal Renderer", no extra flags needed. The
// default headless shell only offers SwiftShader (WebGPU needs --enable-unsafe-webgpu
// there and is then a CPU fallback adapter), so it is not used.
import { join } from "node:path";
import { serve } from "./serve.mjs";

export async function withDemo(root, { width = 1440, height = 900, dpr = 2, query = "scene=none", headless = true } = {}, action) {
  process.env.PLAYWRIGHT_BROWSERS_PATH = join(root, ".runtime/browsers");
  const { chromium } = await import("playwright");
  const server = await serve();
  let browser;
  try {
    browser = await chromium.launch({ channel: "chromium", headless, args: ["--force-color-profile=srgb"] });
    if (browser.version() !== "156.0.8078.4") throw Error(`Unpinned Chromium ${browser.version()}`);
    const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: dpr });
    await context.route("**/*", r => new URL(r.request().url()).origin === server.origin ? r.continue() : r.abort());
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", e => errors.push(String(e)));
    page.on("console", m => { if (m.type() === "error") errors.push(m.text()); });
    await page.goto(`${server.origin}/web/?${query}`);
    await page.waitForFunction(() => window.vello?.ready || window.vello?.error, null, { timeout: 120_000 });
    const failure = await page.evaluate(() => window.vello.error);
    if (failure) throw Error(failure);
    const gpu = await page.evaluate(async () => {
      const adapter = await navigator.gpu?.requestAdapter();
      const gl = document.createElement("canvas").getContext("webgl2");
      const ext = gl?.getExtension("WEBGL_debug_renderer_info");
      return {
        webgpu: adapter ? { vendor: adapter.info.vendor, architecture: adapter.info.architecture,
          fallback: adapter.info.isFallbackAdapter } : null,
        webgl2: gl ? gl.getParameter(ext ? ext.UNMASKED_RENDERER_WEBGL : gl.RENDERER) : null,
      };
    });
    return await action({ page, origin: server.origin, browserVersion: browser.version(), gpu, errors });
  } finally {
    try { await browser?.close(); } finally { await server.close(); }
  }
}
