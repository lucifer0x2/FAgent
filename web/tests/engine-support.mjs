// Shared harness for the renderer=vello browser checks: the in-memory scene fixture,
// Vite on a random loopback port, and the pinned Chromium (full build in new headless
// mode for a real GPU; the headless shell has no usable GPU, which the fallback test uses).
import { spawn } from "node:child_process";
import { once } from "node:events";
import { fileURLToPath } from "node:url";
import { startSceneFixture } from "./scene-wasm-fixture.mjs";

process.env.PLAYWRIGHT_BROWSERS_PATH ??= fileURLToPath(
  new URL("../../.runtime/browsers", import.meta.url),
);

/** Vite (dev, or `preview` of web/dist with `mode: "preview"`) proxied to `api`. */
export async function startVite(api, { mode = "dev" } = {}) {
  const args =
    mode === "preview"
      ? ["node_modules/vite/bin/vite.js", "preview", "--host", "127.0.0.1", "--port", "0", "--strictPort", "false"]
      : ["node_modules/vite/bin/vite.js", "--host", "127.0.0.1", "--port", "0"];
  const child = spawn(process.execPath, args, {
    cwd: fileURLToPath(new URL("../", import.meta.url)),
    env: { ...process.env, FAGENT_API_URL: api },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let output = "";
  child.stdout.on("data", (value) => (output += value));
  child.stderr.on("data", (value) => (output += value));
  for (let i = 0; i < 200 && !output.match(/http:\/\/127\.0\.0\.1:\d+/); i++)
    await new Promise((resolve) => setTimeout(resolve, 50));
  const base = output.match(/http:\/\/127\.0\.0\.1:\d+/)?.[0];
  if (!base) {
    child.kill("SIGTERM");
    throw new Error(`Vite startup: ${output}`);
  }
  return {
    base,
    async close() {
      const exited = once(child, "exit");
      child.kill("SIGTERM");
      await exited;
    },
  };
}

/** `gpu`: full Chromium (WebGPU/WebGL2 on the real GPU); otherwise the headless shell
 *  with GPU and software GL disabled (no WebGPU adapter, no WebGL2 context).
 *  `FAGENT_TEST_CHANNEL=chrome` uses an installed Google Chrome instead of the pinned builds,
 *  for both cases (the GPU-less case then runs Chrome with the same flags). */
export async function launch({ gpu = true } = {}) {
  const { chromium } = await import("playwright");
  const channel = process.env.FAGENT_TEST_CHANNEL;
  return chromium.launch(
    gpu
      ? { channel: channel ?? "chromium", headless: true, args: ["--force-color-profile=srgb"] }
      : { channel, headless: true, args: ["--disable-gpu", "--disable-software-rasterizer"] },
  );
}

/** Collects main-thread long tasks from page start (PerformanceObserver, buffered). */
export const LONG_TASKS = () => {
  window.__longTasks = [];
  try {
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries())
        window.__longTasks.push({
          start: Math.round(entry.startTime),
          duration: Math.round(entry.duration),
        });
    }).observe({ type: "longtask", buffered: true });
  } catch {
    window.__longTasks = null;
  }
};

export async function withFixture(action) {
  const fixture = await startSceneFixture();
  const vite = await startVite(fixture.baseURL);
  try {
    return await action({ fixture, base: vite.base });
  } finally {
    await vite.close();
    await fixture.close();
  }
}
