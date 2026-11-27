// Probes and statistics for engine-load.mjs. LOAD_PROBE runs in the page before any app
// code (init script): it wraps Worker to timestamp the engine protocol on the page clock.
// ENGINE_TIMES is appended to the engine's wasm-bindgen glue in the worker: it totals the
// time of every Engine method and logs the calls that order a load (frame computations,
// commits, draws, cameras), which the runner moves onto the page clock.

/** Page init script: records engine messages as [type, ms, ...] in `window.__load.events`. */
export const LOAD_PROBE = () => {
  const events = [];
  // Start of the first `/snapshot` request (Resource Timing, page clock).
  new PerformanceObserver((list) => {
    for (const entry of list.getEntries())
      if (!events.some((e) => e[0] === "snapshot") && new URL(entry.name).pathname.endsWith("/snapshot"))
        events.push(["snapshot", entry.startTime]);
  }).observe({ type: "resource", buffered: true });
  const load = {
    events,
    /**
     * The latest sync (of `expected` frames when given) is synced and an image was drawn
     * since it was sent: the worker draws a load's last frames right before `synced`, and
     * nothing may draw after it.
     */
    done(expected) {
      const start = events.findLastIndex((e) => e[0] === "sync");
      const last = events[start];
      if (!last || (expected != null && last[2] !== expected)) return false;
      const synced = events.findIndex((e, i) => i > start && e[0] === "synced" && e[2] === last[3]);
      return synced >= 0 && events.slice(start).some((e) => e[0] === "drawn");
    },
  };
  window.__load = load;
  const Base = window.Worker;
  window.Worker = class extends Base {
    constructor(url, options) {
      super(url, options);
      this.engine = String(url).includes("engine");
      if (!this.engine) return;
      events.push(["worker", performance.now()]);
      this.addEventListener("message", ({ data: m }) => {
        const at = performance.now();
        if (m?.type === "frame") events.push(["frame", at, m.frameId, Boolean(m.error), Boolean(m.reused), Boolean(m.restored)]);
        else if (m?.type === "drawn") events.push(["drawn", at, JSON.stringify(m.camera ?? null)]);
        else if (m?.type === "synced") events.push(["synced", at, m.generation, m.error ?? null]);
        else if (m?.type === "ready" || m?.type === "backend") events.push([m.type, at]);
      });
    }
    postMessage(message, transfer) {
      if (this.engine && message?.type === "sync")
        events.push(["sync", performance.now(), message.frames.length, message.generation, Boolean(message.snapshot)]);
      else if (this.engine && message?.type === "layout") events.push(["layout", performance.now()]);
      else if (this.engine && message?.type === "camera")
        events.push(["camera", performance.now(), JSON.stringify(message.camera)]);
      return super.postMessage(message, transfer);
    }
  };
};

/**
 * Appended to the glue module: wraps each Engine method with a timer (worker globals).
 * `__engineLog` holds [name, start, end, detail] (worker clock) for `compute` (frame id),
 * `commit` (frame id when committed, else null), `render` (whether it returned),
 * `set_camera`, `load_font` and `add_image` (resource id), and `drawn` for each image the
 * worker posted (after the post returned; the worker looks `postMessage` up per call).
 */
export const ENGINE_TIMES = `
;{
  const times = (globalThis.__engineTimes = {});
  const log = (globalThis.__engineLog = []);
  const post = globalThis.postMessage;
  globalThis.postMessage = function (message, transfer) {
    const result = post.call(globalThis, message, transfer);
    if (message?.type === "drawn") {
      const at = performance.now();
      log.push(["drawn", at, at, null]);
    }
    return result;
  };
  const detail = {
    compute: (args) => args[1],
    commit: (args, result, ok) => (ok && result ? args[0] : null),
    render: (args, result, ok) => ok,
    set_camera: () => null,
    load_font: (args) => args[0],
    add_image: (args) => args[0],
  };
  const proto = Engine.prototype;
  for (const name of Object.getOwnPropertyNames(proto)) {
    const d = Object.getOwnPropertyDescriptor(proto, name);
    if (name === "constructor" || typeof d?.value !== "function") continue;
    const original = d.value;
    proto[name] = function (...args) {
      const start = performance.now();
      let result;
      let ok = false;
      try {
        result = original.apply(this, args);
        ok = true;
        return result;
      } finally {
        const end = performance.now();
        const ms = end - start;
        const t = (times[name] ??= { n: 0, ms: 0, max: 0 });
        t.n++;
        t.ms += ms;
        if (ms > t.max) t.max = ms;
        if (detail[name]) log.push([name, start, end, detail[name](args, result, ok)]);
      }
    };
  }
}
`;

/** Engine methods whose call counts each run reports (scene work, fonts, cache records). */
const ENGINE_CALLS = ["frame_keys", "compute", "import_record", "export_record", "required_fonts", "load_font", "render"];

const round = (v) => (v == null ? null : Math.round(v));
export const pct = (values, p) => {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))];
};

/**
 * Latency from each frame committed in the worker during the last sync to the delivered
 * `drawn` of the first image whose render started after it. The worker logs each image it
 * posted (`drawn`), right after the render that drew it, so every image carries its own
 * render's start: a render whose image was never posted (it threw after returning) pairs
 * with nothing. Posts and page `drawn` events pair in order (messages keep their order).
 */
function commitLatencies(events, log, syncAt) {
  const drawn = events.filter((e) => e[0] === "drawn").map((e) => e[1]);
  const images = [];
  let renderStart;
  for (const c of log) {
    if (c[0] === "render") renderStart = c[3] ? c[1] : undefined;
    else if (c[0] === "drawn") images.push(renderStart);
  }
  return log
    .filter((c) => c[0] === "commit" && c[3] != null && c[1] >= syncAt)
    .map((commit) => {
      const image = images.findIndex((start) => start != null && start >= commit[2]);
      const at = image >= 0 ? drawn[image] : undefined;
      return at == null ? Infinity : at - commit[2];
    });
}

/**
 * Cameras sent while frames load. Each is applied by the first worker `set_camera` after it
 * was sent; it must be drawn before another frame computation starts. A violation lists the
 * engine calls between the camera and its draw (`late`). The camera → `drawn` latency counts
 * only cameras drawn in order; one never drawn counts as Infinity.
 */
function cameraChecks(events, log, firstFrame, syncedIndex) {
  const applied = log.filter((c) => c[0] === "set_camera");
  const renders = log.filter((c) => c[0] === "render" && c[3]);
  const computes = log.filter((c) => c[0] === "compute").map((c) => c[1]);
  const latencies = [];
  let checked = 0;
  const late = [];
  events.forEach((e, i) => {
    if (e[0] !== "camera" || i <= firstFrame || i >= syncedIndex) return;
    const set = applied.find((c) => c[1] >= e[1]);
    if (set) {
      checked++;
      const next = renders.find((r) => r[1] >= set[2]);
      const between = (start) => start >= set[2] && (!next || start < next[1]);
      if (computes.some(between)) {
        late.push(
          log
            .filter((c) => c[0] !== "render" && c[0] !== "set_camera" && between(c[1]))
            .map((c) => (c[3] == null ? c[0] : `${c[0]}:${c[3]}`))
            .join(" "),
        );
        return;
      }
    }
    const drawn = events.find((d, j) => j > i && d[0] === "drawn" && d[2] === e[2]);
    const later = events.slice(i + 1).some((d) => d[0] === "camera");
    if (drawn) latencies.push(drawn[1] - e[1]);
    else if (!later) latencies.push(Infinity);
  });
  return { latencies, checked, late };
}

const spread = (values) => ({
  p50: round(pct(values, 0.5)),
  p95: round(pct(values, 0.95)),
  max: round(values.length ? Math.max(...values) : null),
});

/**
 * One run's marks (page clock, ms from navigation) and counts. `log` is the worker's
 * `__engineLog` already on the page clock.
 */
export function summarize(events, longTasks, engine, log = []) {
  const first = (type, from = 0) => events.find((e, i) => i >= from && e[0] === type);
  const at = (type, from) => round(first(type, from)?.[1]);
  const syncs = events.filter((e) => e[0] === "sync");
  const last = syncs.at(-1);
  const lastIndex = events.lastIndexOf(last);
  const syncedIndex = events.findIndex((e) => e[0] === "synced" && e[2] === last?.[3]);
  const frames = events.filter((e, i) => e[0] === "frame" && i > lastIndex && i < syncedIndex);
  const firstFrame = events.findIndex((e, i) => e[0] === "frame" && i > lastIndex);
  // Page side: a committed frame's message to the next drawn image. The worker posts that
  // message just before the image showing the frame, so this is ~0 by design; the commit
  // in the worker is what `commitToDrawn` measures from.
  const notified = [];
  events.forEach((e, i) => {
    if (e[0] !== "frame" || e[3] || e[4] || i <= lastIndex) return;
    const drawn = first("drawn", i);
    if (drawn) notified.push(drawn[1] - e[1]);
  });
  const committed = commitLatencies(events, log, last?.[1] ?? 0);
  const draws = events.filter((e, i) => e[0] === "drawn" && i > lastIndex && i <= syncedIndex);
  const cameras = cameraChecks(events, log, firstFrame, syncedIndex);
  const syncAt = first("sync")?.[1] ?? 0;
  const tasks = longTasks.filter((t) => t.start >= 0);
  const engineMs = Object.fromEntries(
    Object.entries(engine ?? {})
      .filter(([, t]) => t.ms >= 1)
      .map(([name, t]) => [name, { n: t.n, ms: round(t.ms), max: round(t.max) }]),
  );
  const synced = events[syncedIndex]?.[1];
  const backend = first("backend")?.[1];
  // The last sync's window in order, consecutive frame messages folded into a count.
  const trace = [];
  for (const e of events.slice(Math.max(lastIndex, 0), syncedIndex + 1)) {
    const top = trace.at(-1);
    if (e[0] === "frame" && top?.[0] === "frames") top[2]++;
    else trace.push(e[0] === "frame" ? ["frames", round(e[1]), 1] : [e[0], round(e[1])]);
  }
  return {
    marks: {
      worker: at("worker"),
      snapshotRequest: at("snapshot"),
      ready: at("ready"),
      backend: at("backend"),
      sync: round(syncAt),
      /** The worker's first `commit` of the last sync (computed or imported frame). */
      firstCommit: round(log.find((c) => c[0] === "commit" && c[3] != null && c[1] >= (last?.[1] ?? 0))?.[2]),
      firstFrame: round(events[firstFrame]?.[1]),
      firstDrawn: firstFrame >= 0 ? at("drawn", firstFrame) : null,
      synced: round(events[syncedIndex]?.[1]),
      /** The image showing the load's last frames: posted right before `synced`, else after. */
      lastDrawn:
        syncedIndex >= 0
          ? round((events.slice(lastIndex, syncedIndex).findLast((e) => e[0] === "drawn") ?? first("drawn", syncedIndex))?.[1])
          : null,
    },
    /** `synced` − `backend`: what the load costs once a backend can draw. */
    syncedAfterBackend: synced != null && backend != null ? round(synced - backend) : null,
    /** Engine calls per method in the whole run, whatever their time (scene work counts). */
    calls: Object.fromEntries(ENGINE_CALLS.map((name) => [name, engine?.[name]?.n ?? 0])),
    syncs: syncs.length,
    frames: frames.length,
    failed: frames.filter((e) => e[3]).length,
    reused: frames.filter((e) => e[4]).length,
    restored: frames.filter((e) => e[5]).length,
    drawsWhileLoading: draws.length,
    trace,
    commitToDrawn: { count: committed.length, ...spread(committed) },
    frameToDrawn: spread(notified),
    cameraToDrawn: {
      count: cameras.latencies.length,
      p50: round(pct(cameras.latencies, 0.5)),
      max: round(cameras.latencies.length ? Math.max(...cameras.latencies) : null),
    },
    /** Cameras applied while loading, and those after which a frame computed before a draw. */
    cameraOrder: { checked: cameras.checked, violations: cameras.late.length, late: cameras.late },
    longTasks: {
      count: tasks.length,
      totalMs: tasks.reduce((s, t) => s + t.duration, 0),
      maxMs: tasks.length ? Math.max(...tasks.map((t) => t.duration)) : 0,
    },
    engineMs,
  };
}

/** p50/min/max of each mark and engine total over runs. */
export function aggregate(runs) {
  const stat = (values) => {
    const v = values.filter((x) => x != null);
    return v.length ? { p50: pct(v, 0.5), min: Math.min(...v), max: Math.max(...v) } : null;
  };
  const marks = Object.fromEntries(
    Object.keys(runs[0].marks).map((k) => [k, stat(runs.map((r) => r.marks[k]))]),
  );
  const names = new Set(runs.flatMap((r) => Object.keys(r.engineMs)));
  const engine = Object.fromEntries(
    [...names].map((name) => [name, stat(runs.map((r) => r.engineMs[name]?.ms))?.p50]),
  );
  const calls = Object.fromEntries(
    ENGINE_CALLS.map((name) => [name, stat(runs.map((r) => r.calls?.[name] ?? 0))]),
  );
  const cacheKeys = [...new Set(runs.flatMap((r) => Object.keys(r.cache ?? {})))].filter(
    (key) => runs.every((r) => typeof (r.cache?.[key] ?? 0) === "number"),
  );
  return {
    marks,
    syncedAfterBackend: stat(runs.map((r) => r.syncedAfterBackend)),
    fontWindowMs: stat(runs.map((r) => r.fonts?.windowMs)),
    fontFetchMs: stat(runs.map((r) => r.fonts?.fetchMs)),
    calls,
    cache: Object.fromEntries(cacheKeys.map((key) => [key, stat(runs.map((r) => r.cache?.[key] ?? 0))])),
    drawsWhileLoading: stat(runs.map((r) => r.drawsWhileLoading)),
    commitToDrawnP95: stat(runs.map((r) => r.commitToDrawn.p95)),
    commitToDrawnMax: stat(runs.map((r) => r.commitToDrawn.max)),
    frameToDrawnP95: stat(runs.map((r) => r.frameToDrawn.p95)),
    cameraToDrawnMax: stat(runs.map((r) => r.cameraToDrawn.max)),
    cameraOrderViolations: runs.reduce((s, r) => s + r.cameraOrder.violations, 0),
    longTasksTotal: stat(runs.map((r) => r.longTasks.totalMs)),
    engineMsP50: engine,
  };
}
