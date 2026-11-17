// Names the engine module by content hash, writes its manifest and compressed sidecars.
// Usage: node scripts/package-engine-wasm.mjs <wasm-bindgen output dir> <public dir>
// → <public>/fagent_engine_wasm-<hash>.js, fagent_engine_wasm_bg-<hash>.wasm (+ .br/.gz)
//   and manifest.json {js, wasm, sizes}; older hashed files there are removed.
// The viewer Worker reads manifest.json (no-store), so a new build is picked up at once and
// the hashed module itself never collides with a stale copy. The server serves these exact
// names (12 lowercase hex) as `immutable` for a year; see docs/VIEWER-CACHE-POLICY.md.
import { createHash } from "node:crypto";
import {
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { brotliCompressSync, constants, gzipSync } from "node:zlib";

export function packageEngineWasm(input, output) {
  const glue = readFileSync(join(input, "fagent_engine_wasm.js"));
  const wasm = readFileSync(join(input, "fagent_engine_wasm_bg.wasm"));
  const hash = createHash("sha256")
    .update(glue)
    .update(wasm)
    .digest("hex")
    .slice(0, 12);
  mkdirSync(output, { recursive: true });
  for (const name of readdirSync(output))
    if (name.startsWith("fagent_engine_wasm") || name === "manifest.json")
      rmSync(join(output, name));
  const files = {
    js: `fagent_engine_wasm-${hash}.js`,
    wasm: `fagent_engine_wasm_bg-${hash}.wasm`,
  };
  const sizes = {};
  for (const [kind, bytes] of [
    ["js", glue],
    ["wasm", wasm],
  ]) {
    const path = join(output, files[kind]);
    const gzip = gzipSync(bytes, { level: 9 });
    const brotli = brotliCompressSync(bytes, {
      params: { [constants.BROTLI_PARAM_QUALITY]: 11 },
    });
    writeFileSync(path, bytes);
    writeFileSync(`${path}.gz`, gzip);
    writeFileSync(`${path}.br`, brotli);
    sizes[kind] = { raw: bytes.length, gzip: gzip.length, brotli: brotli.length };
  }
  writeFileSync(
    join(output, "manifest.json"),
    JSON.stringify({ ...files, sizes }, null, 2) + "\n",
  );
  return { ...files, sizes };
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  if (process.argv.length !== 4)
    throw new Error(
      "Usage: node package-engine-wasm.mjs <bindgen-dir> <public-dir>",
    );
  const result = packageEngineWasm(
    resolve(process.argv[2]),
    resolve(process.argv[3]),
  );
  console.log(JSON.stringify(result));
}
