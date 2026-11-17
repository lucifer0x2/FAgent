// Static loopback server for the Task 29 demo: serves spikes/vello-gpu/ (web/, pkg/, out/)
// on an ephemeral 127.0.0.1 port (a secure context, so WebGPU is exposed).
// Usage: node spikes/vello-gpu/serve.mjs [port]  → open <url>/web/?renderer=webgpu
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { dirname, extname, join, normalize, sep } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const types = {
  ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".json": "application/json",
  ".wasm": "application/wasm", ".png": "image/png", ".ttf": "font/ttf",
};

export async function serve(port = 0) {
  const server = createServer(async (request, response) => {
    const path = decodeURIComponent(new URL(request.url, "http://x").pathname);
    const file = normalize(join(here, path.endsWith("/") ? `${path}index.html` : path));
    if (!file.startsWith(here + sep)) return response.writeHead(403).end();
    try {
      const bytes = await readFile(file);
      response.writeHead(200, {
        "content-type": types[extname(file)] ?? "application/octet-stream",
        "cache-control": "no-store",
      }).end(bytes);
    } catch {
      response.writeHead(404).end();
    }
  });
  await new Promise((ok, fail) => { server.once("error", fail); server.listen(port, "127.0.0.1", ok); });
  return {
    origin: `http://127.0.0.1:${server.address().port}`,
    close: () => new Promise(ok => { server.closeAllConnections(); server.close(() => ok()); }),
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const { origin } = await serve(Number(process.argv[2] ?? 0));
  console.log(`${origin}/web/?renderer=webgpu&scene=/out/board/board.json`);
}
