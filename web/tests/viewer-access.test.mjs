import assert from "node:assert/strict";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";

process.env.PLAYWRIGHT_BROWSERS_PATH ??= fileURLToPath(new URL("../../.runtime/browsers", import.meta.url));
const { chromium } = await import("playwright");

test("viewer capability shares its pending read, fails closed and reports live updates", async () => {
  const vite = await createServer({
    root: fileURLToPath(new URL("../", import.meta.url)),
    server: { host: "127.0.0.1", port: 0 }, logLevel: "error",
  });
  let browser;
  try {
    await vite.listen();
    browser = await chromium.launch({ headless: true });
    // An older server omits liveUpdates: streams stay on. A public listener may turn them off.
    const bodies = {
      writable: { readOnly: false },
      "read only": { readOnly: true, liveUpdates: false },
      "read only live": { readOnly: true, liveUpdates: true },
    };
    const lives = { writable: "live", "read only": "static", "read only live": "live", failure: "live" };
    for (const mode of ["writable", "read only", "read only live", "failure"]) {
      const page = await browser.newPage();
      page.setDefaultTimeout(5000);
      let requests = 0;
      let release;
      let requestStarted;
      const started = new Promise((resolve) => { requestStarted = resolve; });
      await page.route("**/viewer-access-test", (route) => route.fulfill({
        contentType: "text/html", body: '<div id="root"></div><script type="module" src="/tests/viewer-access-fixture.tsx"></script>',
      }));
      await page.route("**/api/v1/viewer", async (route) => {
        // A failed probe is retried with backoff; the retry reaches a public static listener.
        if (++requests > 1) return route.fulfill({ json: bodies["read only"] });
        await new Promise((resolve) => { release = resolve; requestStarted(); });
        await route.fulfill(mode === "failure"
          ? { status: 503, json: { error: { message: "Unavailable" } } }
          : { json: bodies[mode] });
      });
      await page.goto(`${vite.resolvedUrls.local[0]}viewer-access-test`);
      const first = page.getByLabel("first", { exact: true });
      await first.getByText("pending", { exact: true }).waitFor();
      await page.getByLabel("second", { exact: true }).getByText("pending", { exact: true }).waitFor();
      await page.getByLabel("first live").getByText("pending", { exact: true }).waitFor();
      await started;
      assert.equal(requests, 1, "concurrent StrictMode consumers share the capability read");
      release();
      const expected = mode === "writable" ? "writable" : "read only";
      await first.getByText(expected, { exact: true }).waitFor();
      await page.getByLabel("second", { exact: true }).getByText(expected, { exact: true }).waitFor();
      for (const name of ["first live", "second live"])
        await page.getByLabel(name).getByText(lives[mode], { exact: true }).waitFor();
      if (mode === "failure") {
        // One transient failure does not pin the guess: the retry settles on the listener's answer.
        for (const name of ["first live", "second live"])
          await page.getByLabel(name).getByText("static", { exact: true }).waitFor();
        assert.equal(requests, 2);
      } else assert.equal(requests, 1);
      await page.close();
    }
  } finally {
    await browser?.close();
    await vite.close();
  }
});

test("a denied stream re-reads the probe and stops reconnecting when live updates are off", async () => {
  const vite = await createServer({
    root: fileURLToPath(new URL("../", import.meta.url)),
    server: { host: "127.0.0.1", port: 0 }, logLevel: "error",
  });
  let browser;
  try {
    await vite.listen();
    browser = await chromium.launch({ headless: true });
    const page = await browser.newPage();
    page.setDefaultTimeout(5000);
    let probes = 0;
    let streams = 0;
    await page.route("**/viewer-live-test", (route) => route.fulfill({
      contentType: "text/html", body: '<div id="root"></div><script type="module" src="/tests/viewer-live-denied-fixture.tsx"></script>',
    }));
    // The first probe fails in a way that is not retried, so only the denied stream can re-read it.
    await page.route("**/api/v1/viewer", (route) => route.fulfill(++probes === 1
      ? { status: 404, json: { error: { code: "NOT_FOUND", message: "Missing" } } }
      : { json: { readOnly: true, liveUpdates: false } }));
    await page.route("**/api/v1/events", (route) => {
      streams++;
      return route.fulfill({
        status: 403,
        json: { error: { code: "PUBLIC_LIVE_DISABLED", message: "Live updates are off." } },
      });
    });
    await page.goto(`${vite.resolvedUrls.local[0]}viewer-live-test`);
    await page.getByLabel("connection").getByText("static", { exact: true }).waitFor();
    const opened = streams;
    assert.ok(opened >= 1, "the failed probe guessed live and opened a stream");
    assert.equal(probes, 2, "the denied stream re-read the capability probe");
    await page.waitForTimeout(1500);
    assert.equal(streams, opened, "no reconnect after the listener said live updates are off");
    await page.getByLabel("connection").getByText("static", { exact: true }).waitFor();
  } finally {
    await browser?.close();
    await vite.close();
  }
});
