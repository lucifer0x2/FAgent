// renderer=vello in the real viewer (Task 30): engine Worker compute + paint on WebGPU,
// forced WebGL2 / vello_cpu, GPU failure → vello_cpu (headless shell has no usable GPU),
// module failure → Canvas2D, WebGPU device loss (rebuild once, then WebGL2), hit test and
// Inspector parity with Canvas2D, a screenshot diff against Canvas2D, and main-thread long
// tasks while loading, panning and zooming, and overscan (a pan within the margin while the
// worker cannot redraw shows drawn pixels). Temporary in-memory fixture, loopback ports.
// Overscan is off by default (FAGENT_VIEWER_OVERSCAN); the Vello page gets ratio 0.1 by
// fulfilling its /api/v1/viewer probe with `overscan: 0.1` added (`OVERSCAN_10`).
import assert from "node:assert/strict";
import { decodePNG } from "../../spikes/native-paint-parity/png.mjs";
import { launch, LONG_TASKS, withFixture } from "./engine-support.mjs";

const VIEWPORT = { width: 1440, height: 1000 };
const results = {};

/** Fraction of pixels whose RGB differs by more than 8 between two PNG screenshots. */
function differing(a, b) {
  const x = decodePNG(a),
    y = decodePNG(b);
  assert.equal(x.width, y.width);
  let over = 0;
  for (let i = 0; i < x.data.length; i += 4)
    if (
      Math.abs(x.data[i] - y.data[i]) > 8 ||
      Math.abs(x.data[i + 1] - y.data[i + 1]) > 8 ||
      Math.abs(x.data[i + 2] - y.data[i + 2]) > 8
    )
      over++;
  return over / (x.width * x.height);
}

/**
 * Test hook: `__holdCamera(true)` keeps camera messages from reaching the engine Worker
 * (as when it is busy computing a frame), so the page shows its last image moved by the
 * stale placement; `__holdCamera(false)` delivers the latest held camera.
 */
const HOLD_CAMERA = () => {
  const post = Worker.prototype.postMessage;
  let hold = false,
    held;
  window.__holdCamera = (on) => {
    hold = on;
    if (!on && held) post.call(...held);
    held = undefined;
  };
  Worker.prototype.postMessage = function (message, transfer) {
    if (hold && message?.type === "camera") return void (held = [this, message]);
    return post.call(this, message, transfer);
  };
};

/** Adds `overscan: 0.1` to the listener's capabilities, as FAGENT_VIEWER_OVERSCAN=10 would. */
const OVERSCAN_10 = (context) =>
  context.route("**/api/v1/viewer", async (route) => {
    const response = await route.fetch();
    await route.fulfill({ response, json: { ...(await response.json()), overscan: 0.1 } });
  });

/** Fraction of pixels in `[x, y, width, height]` (CSS px = PNG px) differing by > 8. */
function differingIn(a, b, [x0, y0, width, height]) {
  const x = decodePNG(a),
    y = decodePNG(b);
  let over = 0;
  for (let row = y0; row < y0 + height; row++)
    for (let col = x0; col < x0 + width; col++) {
      const i = (row * x.width + col) * 4;
      if ([0, 1, 2].some((c) => Math.abs(x.data[i + c] - y.data[i + c]) > 8)) over++;
    }
  return over / (width * height);
}

/** Fraction of pixels in a region that show the bare pane background (#edf0f3). */
function backgroundIn(png, [x0, y0, width, height]) {
  const { data, width: stride } = decodePNG(png);
  let bare = 0;
  for (let row = y0; row < y0 + height; row++)
    for (let col = x0; col < x0 + width; col++) {
      const i = (row * stride + col) * 4;
      if ([0xed, 0xf0, 0xf3].every((v, c) => Math.abs(data[i + c] - v) <= 3)) bare++;
    }
  return bare / (width * height);
}

async function open(browser, base, query, setup) {
  const context = await browser.newContext({ viewport: VIEWPORT });
  await context.addInitScript(LONG_TASKS);
  await setup?.(context);
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  // Name the page: with no page the engine opens the All pages board, which Canvas2D lacks.
  await page.goto(`${base}/?project=fixture&page=page&${query}`);
  return { context, page, errors };
}

const status = (page) => page.evaluate(() => window.fagentEngine.status());
const state = (page) => page.evaluate(() => window.fagentEngine.state());
const selected = (page) => new URL(page.url()).searchParams.get("node") ?? "";
const host = (page) => page.locator(".canvas-host");

async function drawn(page) {
  for (let i = 0; i < 100; i++) {
    const value = await status(page);
    if (value?.lastRender?.frames > 0 && value.idle) return value;
    await page.waitForTimeout(50);
  }
  throw new Error("engine never drew a frame");
}

/** Clicks host-relative points; returns the selected node id after each click. */
async function clicks(page, points) {
  const box = await host(page).boundingBox();
  const chosen = [];
  for (const [x, y] of points) {
    const before = page.url();
    await page.mouse.click(box.x + x, box.y + y);
    for (let i = 0; i < 40 && page.url() === before; i++) await page.waitForTimeout(25);
    chosen.push(selected(page));
  }
  return chosen;
}

const POINTS = [
  [120, 160], // description over the headline
  [100, 132], // frame label strip (no node)
  [500, 500], // frame body
  [20, 20], // outside every frame
  [640, 175], // headline again
];

await withFixture(async ({ fixture, base }) => {
  let browser = await launch({ gpu: true });
  try {
    // Canvas2D reference: selection, inspector text and a screenshot.
    const canvas = await open(browser, base, "renderer=canvas2d");
    await canvas.page.getByText("Scene r7", { exact: true }).waitFor();
    await canvas.page.waitForTimeout(300);
    const canvasShot = await host(canvas.page).screenshot();
    const canvasPicks = await clicks(canvas.page, POINTS);
    await clicks(canvas.page, [POINTS[0]]);
    const canvasBounds = await canvas.page.locator(".bounds-grid").innerText();
    results.canvas2dLongTasks = await canvas.page.evaluate(() => window.__longTasks);
    await canvas.context.close();

    const before = fixture.requests.length;
    const vello = await open(browser, base, "renderer=vello&engineDebug=1", (context) =>
      Promise.all([context.addInitScript(HOLD_CAMERA), OVERSCAN_10(context)]),
    );
    const { page } = vello;
    await page.getByText("Scene r7", { exact: true }).waitFor();
    await page.getByText("Vello · WebGPU").waitFor();
    const first = await drawn(page);
    assert.equal(first.backend, "webgpu");
    const requests = fixture.requests.slice(before);
    assert.ok(!requests.some((url) => url.includes("/scene?")), "no server scene");
    const shot = await host(page).screenshot();
    results.screenshotDiffVsCanvas2d = differing(canvasShot, shot);
    assert.ok(
      results.screenshotDiffVsCanvas2d < 0.01,
      `viewer differs from Canvas2D in ${results.screenshotDiffVsCanvas2d}`,
    );
    const picks = await clicks(page, POINTS);
    assert.deepEqual(picks, canvasPicks, "hit test matches Canvas2D");
    assert.ok(picks.includes("title") && picks.includes("frame") && picks.includes(""));
    await clicks(page, [POINTS[0]]);
    await page.locator(".bounds-grid").waitFor();
    assert.equal(await page.locator(".bounds-grid").innerText(), canvasBounds);
    await page.getByText("web-4").waitFor();

    // Overscan: the image covers the viewport plus 10% per side, placed at −margin.
    const box = await host(page).boundingBox();
    // The ratio arrives with the capability probe, possibly after the first image: wait for
    // a render with it.
    let grown = await drawn(page);
    for (let i = 0; i < 100; i++) {
      if (grown.overscan.ratio === 0.1 && grown.lastRender.width > Math.round(box.width)) break;
      await page.waitForTimeout(50);
      grown = await drawn(page);
    }
    const overscan = grown.overscan;
    results.overscan = overscan;
    assert.equal(overscan.ratio, 0.1, "the ratio from /api/v1/viewer reached the worker");
    assert.equal(grown.lastRender.width, Math.round(box.width) + 2 * overscan.x);
    assert.equal(grown.lastRender.height, Math.round(box.height) + 2 * overscan.y);
    assert.equal(overscan.x, Math.round(0.1 * Math.round(box.width)));
    assert.equal(overscan.y, Math.round(0.1 * Math.round(box.height)));
    assert.ok(overscan.maxTexture >= 4096, `max texture ${overscan.maxTexture}`);
    const clipped = await host(page).evaluate((element) => {
      element.scrollLeft = 50;
      element.scrollTop = 50;
      return [getComputedStyle(element).overflow, element.scrollLeft, element.scrollTop];
    });
    assert.deepEqual(clipped, ["clip", 0, 0], "the host clips the overscan, never scrolls");
    // Zoom in until the white frame fills the viewport, then pan by less than the margin
    // while the worker cannot redraw: the strip that slides in shows drawn frame pixels,
    // the same as once the worker draws the new camera.
    await host(page).focus();
    for (let i = 0; i < 4; i++) await page.keyboard.press("+");
    await drawn(page);
    const [dx, dy] = [Math.round(overscan.x * 0.7), Math.round(overscan.y * 0.7)];
    await page.evaluate(() => window.__holdCamera(true));
    const rendersBefore = (await status(page)).renders.length;
    await page.mouse.move(box.x + 500, box.y + 400);
    await page.mouse.down();
    for (let i = 1; i <= 10; i++)
      await page.mouse.move(box.x + 500 - (dx * i) / 10, box.y + 400 - (dy * i) / 10);
    await page.mouse.up();
    await page.waitForTimeout(100);
    assert.equal((await status(page)).renders.length, rendersBefore, "the worker did not redraw");
    const held = await host(page).screenshot();
    await page.evaluate(() => window.__holdCamera(false));
    await page.waitForTimeout(100);
    await drawn(page);
    const redrawn = await host(page).screenshot();
    const width = Math.floor(box.width),
      height = Math.floor(box.height);
    const right = [width - dx, 0, dx, height],
      bottom = [0, height - dy, width, dy];
    results.overscanStrip = {
      pan: [dx, dy],
      background: [backgroundIn(held, right), backgroundIn(held, bottom)],
      diffVsRedrawn: [differingIn(held, redrawn, right), differingIn(held, redrawn, bottom)],
      wholeDiffVsRedrawn: differing(held, redrawn),
    };
    for (const v of results.overscanStrip.background)
      assert.ok(v < 0.01, `exposed strip shows the bare background: ${v}`);
    for (const v of results.overscanStrip.diffVsRedrawn)
      assert.ok(v < 0.01, `exposed strip differs from the redrawn image: ${v}`);
    assert.ok(results.overscanStrip.wholeDiffVsRedrawn < 0.01);
    await host(page).focus();
    await page.keyboard.press("f");
    await drawn(page);

    // Pan and zoom: the engine follows the camera; nothing blocks the main thread.
    await page.mouse.move(box.x + 900, box.y + 600);
    await page.mouse.down();
    for (let i = 1; i <= 30; i++)
      await page.mouse.move(box.x + 900 - i * 8, box.y + 600 - i * 4);
    await page.mouse.up();
    assert.equal(selected(page), canvasPicks[0], "a pan does not change selection");
    for (let i = 0; i < 20; i++) {
      await page.mouse.wheel(0, i < 10 ? -60 : 80);
      await page.waitForTimeout(16);
    }
    const moved = await drawn(page);
    assert.notEqual(moved.lastRender.scale, first.lastRender.scale);
    fixture.update(8);
    await page.getByText("Scene r8", { exact: true }).waitFor();
    await page.waitForTimeout(300);
    results.velloLongTasks = await page.evaluate(() => window.__longTasks);
    assert.ok(Array.isArray(results.velloLongTasks), "longtask observer available");
    const worst = Math.max(0, ...results.velloLongTasks.map((t) => t.duration));
    results.velloWorstLongTaskMs = worst;
    assert.ok(worst <= 50, `main-thread long task ${worst} ms`);
    results.lastRender = moved.lastRender;

    // Device loss: rebuilt once on the same canvas, then WebGL2 for the session.
    await page.evaluate(() => window.fagentEngine.loseDevice());
    for (let i = 0; i < 100; i++) {
      if ((await state(page)).notes.some((n) => n.includes("rebuilt once"))) break;
      await page.waitForTimeout(50);
    }
    assert.equal((await state(page)).backend, "webgpu");
    await page.evaluate(() => window.fagentEngine.loseDevice());
    await page.getByText("Vello · WebGL2").waitFor();
    assert.equal((await drawn(page)).backend, "webgl2");
    assert.deepEqual(vello.errors, []);
    await vello.context.close();

    let webgl2Shot;
    for (const [query, label, backend] of [
      ["renderer=webgl2", "Vello · WebGL2", "webgl2"],
      ["renderer=cpu", "Vello · vello_cpu", "cpu"],
    ]) {
      const forced = await open(browser, base, `${query}&engineDebug=1`);
      await forced.page.getByText(label).waitFor();
      await forced.page.getByText("Scene r8", { exact: true }).waitFor();
      assert.equal((await drawn(forced.page)).backend, backend);
      if (backend === "webgl2") webgl2Shot = await host(forced.page).screenshot();
      const diff = differing(
        await host(forced.page).screenshot(),
        await host(forced.page).screenshot(),
      );
      assert.equal(diff, 0, "stable picture");
      results[`${backend}Picks`] = await clicks(forced.page, POINTS.slice(0, 3));
      await forced.context.close();
    }

    // WebGPU that draws nothing or errors (Safari 26 symptom): WebGL2 for the session, with
    // the reason in the notes and the same picture as forced WebGL2.
    for (const [fault, note] of [
      ["blank", /^WebGPU unavailable: present probe: canvas stayed blank .*; trying WebGL2$/],
      ["throw", /^WebGPU unavailable: render: simulated WebGPU render failure; trying WebGL2$/],
      ["error", /^WebGPU unavailable: GPU error: .*; trying WebGL2$/s],
    ]) {
      const faulty = await open(
        browser,
        base,
        `renderer=vello&engineDebug=1${fault === "error" ? "" : `&gpuFault=${fault}`}`,
      );
      if (fault === "error") {
        // A real validation error after WebGPU has drawn: caught by the error poll.
        await faulty.page.getByText("Vello · WebGPU").waitFor();
        assert.equal((await drawn(faulty.page)).backend, "webgpu");
        await faulty.page.evaluate(() => window.fagentEngine.gpuFault("error"));
      }
      await faulty.page.getByText("Vello · WebGL2").waitFor();
      await faulty.page.getByText("Scene r8", { exact: true }).waitFor();
      assert.equal((await drawn(faulty.page)).backend, "webgl2");
      const notes = (await state(faulty.page)).notes;
      assert.equal(notes.length, 1, JSON.stringify(notes));
      assert.match(notes[0], note);
      results[`${fault}FaultNote`] = notes[0];
      const diff = differing(webgl2Shot, await host(faulty.page).screenshot());
      results[`${fault}FaultDiffVsWebgl2`] = diff;
      assert.ok(diff < 0.01, `${fault} fallback picture differs from WebGL2 in ${diff}`);
      assert.deepEqual(faulty.errors, []);
      await faulty.context.close();
    }

    // Module failure: Canvas2D for the session, with a visible reason.
    const broken = await open(browser, base, "renderer=vello", (context) =>
      context.route("**/engine-wasm/manifest.json", (route) =>
        route.fulfill({ status: 404, body: "missing" }),
      ),
    );
    await broken.page.getByText("Scene r8", { exact: true }).waitFor();
    await broken.page
      .getByText(/Vello renderer unavailable; using Canvas2D: Engine module unavailable/)
      .waitFor();
    assert.equal(await broken.page.getByText(/Vello ·/).count(), 0);
    assert.equal(await broken.page.locator(".konvajs-content").count(), 1);
    await broken.context.close();
  } finally {
    await browser.close();
  }

  // No usable GPU (headless shell: no WebGPU adapter, WebGL2 only with a performance
  // caveat): the chain ends on vello_cpu and records why.
  browser = await launch({ gpu: false });
  try {
    const cpu = await open(browser, base, "renderer=vello&engineDebug=1");
    await cpu.page.getByText("Vello · vello_cpu").waitFor({ timeout: 30_000 });
    await cpu.page.getByText("Scene r8", { exact: true }).waitFor();
    const notes = (await state(cpu.page)).notes;
    assert.match(notes[0], /^WebGPU unavailable: .*; trying WebGL2$/);
    assert.match(notes[1], /^WebGL2 unavailable: .*; trying vello_cpu$/);
    results.noGpuNotes = notes;
    results.noGpuPicks = await clicks(cpu.page, POINTS.slice(0, 3));
    assert.deepEqual(results.noGpuPicks, results.cpuPicks);
    await cpu.context.close();
  } finally {
    await browser.close();
  }
});

console.log(JSON.stringify(results, null, 1));
console.log(
  "Engine smoke passed: WebGPU worker paint, Canvas2D hit-test/Inspector/screenshot parity, pan/zoom without long tasks, device loss → rebuild → WebGL2, blank/throwing/erroring WebGPU → WebGL2, forced WebGL2/vello_cpu, no-GPU → vello_cpu, module failure → Canvas2D.",
);
