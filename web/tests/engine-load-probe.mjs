// Probes and statistics for engine-load.mjs. LOAD_PROBE runs in the page before any app
// code (init script): it wraps Worker to timestamp the engine protocol on the page clock.
// ENGINE_TIMES is appended to the engine's wasm-bindgen glue in the worker and totals the
// time of every Engine method.

/** Page init script: records engine messages as [type, ms, ...] in `window.__load.events`. */
export const LOAD_PROBE = () => {
  const events = [];
  const load = {
    events,
    /** The latest sync (of `expected` frames when given) is synced and drawn after. */
    done(expected) {
      const syncs = events.filter((e) => e[0] === "sync");
      const last = syncs.at(-1);
      if (!last || (expected != null && last[2] !== expected)) return false;
      const synced = events.findIndex((e) => e[0] === "synced" && e[2] === last[3]);
      return synced >= 0 && events.slice(synced).some((e) => e[0] === "drawn");
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
        if (m?.type === "frame") events.push(["frame", at, m.frameId, Boolean(m.error), Boolean(m.reused)]);
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

/** Appended to the glue module: wraps each Engine method with a timer (worker global). */
export const ENGINE_TIMES = `
;{
  const times = (globalThis.__engineTimes = {});
  const proto = Engine.prototype;
  for (const name of Object.getOwnPropertyNames(proto)) {
    const d = Object.getOwnPropertyDescriptor(proto, name);
    if (name === "constructor" || typeof d?.value !== "function") continue;
    const original = d.value;
    proto[name] = function (...args) {
      const start = performance.now();
      try {
        return original.apply(this, args);
      } finally {
        const ms = performance.now() - start;
        const t = (times[name] ??= { n: 0, ms: 0, max: 0 });
        t.n++;
        t.ms += ms;
        if (ms > t.max) t.max = ms;
      }
    };
  }
}
`;

const round = (v) => (v == null ? null : Math.round(v));
export const pct = (values, p) => {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))];
};

/** One run's marks (page clock, ms from navigation) and counts. */
export function summarize(events, longTasks, engine) {
  const first = (type, from = 0) => events.find((e, i) => i >= from && e[0] === type);
  const at = (type, from) => round(first(type, from)?.[1]);
  const syncs = events.filter((e) => e[0] === "sync");
  const last = syncs.at(-1);
  const lastIndex = events.lastIndexOf(last);
  const syncedIndex = events.findIndex((e) => e[0] === "synced" && e[2] === last?.[3]);
  const frames = events.filter((e, i) => e[0] === "frame" && i > lastIndex && i < syncedIndex);
  const firstFrame = events.findIndex((e, i) => e[0] === "frame" && i > lastIndex);
  // Latency from each committed (not reused, not failed) frame to the next drawn image.
  const latencies = [];
  events.forEach((e, i) => {
    if (e[0] !== "frame" || e[3] || e[4] || i <= lastIndex) return;
    const drawn = first("drawn", i);
    if (drawn) latencies.push(drawn[1] - e[1]);
  });
  const draws = events.filter((e, i) => e[0] === "drawn" && i > lastIndex && i <= syncedIndex);
  // A camera sent while frames load must be drawn before the next frame, not after the load.
  const cameraLatencies = [];
  events.forEach((e, i) => {
    if (e[0] !== "camera" || i <= firstFrame || i >= syncedIndex) return;
    const drawn = events.find((d, j) => j > i && d[0] === "drawn" && d[2] === e[2]);
    const later = events.slice(i + 1).some((d) => d[0] === "camera");
    if (drawn) cameraLatencies.push(drawn[1] - e[1]);
    else if (!later) cameraLatencies.push(Infinity);
  });
  const syncAt = first("sync")?.[1] ?? 0;
  const tasks = longTasks.filter((t) => t.start >= 0);
  const engineMs = Object.fromEntries(
    Object.entries(engine ?? {})
      .filter(([, t]) => t.ms >= 1)
      .map(([name, t]) => [name, { n: t.n, ms: round(t.ms), max: round(t.max) }]),
  );
  return {
    marks: {
      worker: at("worker"),
      ready: at("ready"),
      backend: at("backend"),
      sync: round(syncAt),
      firstFrame: round(events[firstFrame]?.[1]),
      firstDrawn: firstFrame >= 0 ? at("drawn", firstFrame) : null,
      synced: round(events[syncedIndex]?.[1]),
      lastDrawn: syncedIndex >= 0 ? at("drawn", syncedIndex) : null,
    },
    syncs: syncs.length,
    frames: frames.length,
    failed: frames.filter((e) => e[3]).length,
    reused: frames.filter((e) => e[4]).length,
    drawsWhileLoading: draws.length,
    commitToDrawn: {
      p50: round(pct(latencies, 0.5)),
      p95: round(pct(latencies, 0.95)),
      max: round(latencies.length ? Math.max(...latencies) : null),
    },
    cameraToDrawn: {
      count: cameraLatencies.length,
      p50: round(pct(cameraLatencies, 0.5)),
      max: round(cameraLatencies.length ? Math.max(...cameraLatencies) : null),
    },
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
  return {
    marks,
    drawsWhileLoading: stat(runs.map((r) => r.drawsWhileLoading)),
    commitToDrawnP95: stat(runs.map((r) => r.commitToDrawn.p95)),
    cameraToDrawnMax: stat(runs.map((r) => r.cameraToDrawn.max)),
    longTasksTotal: stat(runs.map((r) => r.longTasks.totalMs)),
    engineMsP50: engine,
  };
}
