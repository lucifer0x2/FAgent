// Exports the test:repaint board (12 frames, ~4 945 nodes) as one world-space scene
// for the Vello measurements: temporary backend + SQLite root (removed afterwards),
// scenes from GET /scenes, frames laid out left to right with a 200 px gap.
// Usage: REPAINT_SERVER_BIN=<release fagent-server> node spikes/vello-gpu/export-board.mjs
//   →  spikes/vello-gpu/out/board/{board.json,resources/}
// Scene jobs run on the native backend (RENDER_BACKEND=vello) and also paint a PNG: a debug
// server exceeds the 45 s job deadline on these 1440×4000 frames, so use a release build
// (cargo build --release --locked -p fagent-server; ~3.5 s per frame).
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { seedProject, startBackend } from "../../web/tests/repaint-fixture.mjs";

const here = fileURLToPath(new URL(".", import.meta.url));
const out = join(here, "out/board");
const GAP = 200;

const backend = await startBackend();
try {
  const project = await seedProject(backend.base);
  const api = path => fetch(`${backend.base}/api/v1${path}`).then(async r => {
    if (!r.ok) throw Error(`${path}: ${r.status} ${await r.text()}`);
    return r;
  });
  const scenes = [];
  for (let offset = 0; offset < 12; offset += 4) {
    const query = `pageId=page&revision=${project.revision}&offset=${offset}&limit=4`;
    const batch = await (await api(`/projects/${project.projectId}/scenes?${query}`)).json();
    for (const job of batch.jobs ?? batch.scenes ?? []) {
      let current = job;
      for (let i = 0; current.status === "pending" && i < 600; i++) {
        await new Promise(r => setTimeout(r, 100));
        current = await (await api(`/jobs/${job.jobId}`)).json();
      }
      if (current.status !== "ready") throw Error(`job ${job.jobId}: ${JSON.stringify(current).slice(0, 400)}`);
      scenes.push(current.scene);
    }
  }
  await mkdir(join(out, "resources"), { recursive: true });
  const fonts = new Map(), assets = new Map();
  let x = 0, height = 0;
  const nodes = [];
  for (const scene of scenes) {
    for (const n of scene.nodes) nodes.push({ ...n, bounds: { ...n.bounds, x: n.bounds.x + x } });
    for (const f of scene.fonts) fonts.set(f.id, f);
    for (const a of scene.assets) assets.set(a.id, a);
    const v = scene.provenance.viewport;
    x += v.width + GAP;
    height = Math.max(height, v.height);
  }
  const local = async (entry, ext) => {
    const bytes = Buffer.from(await (await fetch(new URL(entry.url, backend.base))).arrayBuffer());
    const name = `${entry.id.replace(/[^A-Za-z0-9_-]/g, "_")}.${ext}`;
    await writeFile(join(out, "resources", name), bytes);
    return { ...entry, url: `/resources/${name}` };
  };
  const board = {
    ...scenes[0],
    provenance: { ...scenes[0].provenance, frameId: "board", viewport: { width: x - GAP, height } },
    nodes,
    fonts: await Promise.all([...fonts.values()].map(f => local(f, "font"))),
    assets: await Promise.all([...assets.values()].map(a => local(a, "bin"))),
  };
  await writeFile(join(out, "board.json"), JSON.stringify(board));
  console.log(JSON.stringify({ frames: scenes.length, nodes: nodes.length, viewport: board.provenance.viewport,
    fonts: board.fonts.length, assets: board.assets.length }));
  await project.close();
} finally {
  await backend.stop();
}
