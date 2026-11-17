// Release wasm32 build of the Task 29 demo crate + wasm-bindgen (--target web).
// Profiles: "speed" (opt-level 3, thin LTO) used for the measurements, "size"
// (opt-level s, fat LTO, 1 codegen unit) and "scalar" (speed without SIMD); all use
// panic=abort, like scene-wasm.
// Usage: node spikes/vello-gpu/build.mjs [--profile speed|size|scalar|both|all]  (default both)
//   → spikes/vello-gpu/pkg/<profile>/ and pkg/sizes.json (raw/gzip/brotli bytes)
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { brotliCompressSync, constants, gzipSync } from "node:zlib";

const here = dirname(fileURLToPath(import.meta.url)), root = resolve(here, "../..");
const VERSION = "wasm-bindgen 0.2.129";
const arg = process.argv.indexOf("--profile");
const wanted = arg > 0 ? process.argv[arg + 1] : "both";
// simd128 lets vello_common/fearless_simd use wasm SIMD (every WebGPU/WebGL2 browser has it);
// "scalar" is the speed profile without it, kept for comparison.
const profiles = {
  speed: { OPT_LEVEL: "3", LTO: "thin", CODEGEN_UNITS: "16", SIMD: true },
  size: { OPT_LEVEL: "s", LTO: "true", CODEGEN_UNITS: "1", SIMD: true },
  scalar: { OPT_LEVEL: "3", LTO: "thin", CODEGEN_UNITS: "16", SIMD: false },
};
const selected = wanted === "both" ? ["speed", "size"] : wanted === "all" ? Object.keys(profiles) : [wanted];
if (!selected.every(p => profiles[p])) throw Error(`unknown profile ${wanted}`);

const bindgen = process.env.WASM_BINDGEN ?? join(root, "target/wasm-tools/bin/wasm-bindgen");
const version = spawnSync(bindgen, ["--version"], { encoding: "utf8" });
if (version.stdout?.trim() !== VERSION) throw Error(`requires ${VERSION} at ${bindgen} (or WASM_BINDGEN)`);

function run(command, args, env = {}) {
  const r = spawnSync(command, args, { cwd: root, stdio: "inherit", env: { ...process.env, ...env } });
  if (r.status !== 0) throw Error(`${command} exited ${r.status}`);
}

const sizesFile = join(here, "pkg/sizes.json");
const sizes = existsSync(sizesFile) ? JSON.parse(readFileSync(sizesFile, "utf8")) : {};
for (const name of selected) {
  const p = profiles[name];
  const targetDir = join(root, "target/vello-gpu", name);
  const started = Date.now();
  run("cargo", ["build", "--locked", "--release", "-p", "fagent-vello-gpu-demo",
    "--target", "wasm32-unknown-unknown", "--target-dir", targetDir], {
    CARGO_PROFILE_RELEASE_OPT_LEVEL: p.OPT_LEVEL,
    CARGO_PROFILE_RELEASE_LTO: p.LTO,
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS: p.CODEGEN_UNITS,
    CARGO_PROFILE_RELEASE_PANIC: "abort",
    CARGO_PROFILE_RELEASE_DEBUG: "false",
    RUSTFLAGS: p.SIMD ? "-C target-feature=+simd128" : "",
  });
  const out = join(here, "pkg", name);
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  run(bindgen, [join(targetDir, "wasm32-unknown-unknown/release/fagent_vello_gpu_demo.wasm"),
    "--target", "web", "--out-dir", out, "--out-name", "vello_demo"]);
  rmSync(join(out, "package.json"), { force: true });
  const wasm = readFileSync(join(out, "vello_demo_bg.wasm"));
  const glue = readFileSync(join(out, "vello_demo.js"));
  sizes[name] = {
    ...p,
    buildSeconds: Math.round((Date.now() - started) / 1000),
    wasm: {
      raw: statSync(join(out, "vello_demo_bg.wasm")).size,
      gzip: gzipSync(wasm, { level: 9 }).length,
      brotli: brotliCompressSync(wasm, { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }).length,
    },
    glueJs: { raw: glue.length, brotli: brotliCompressSync(glue).length },
  };
  console.log(JSON.stringify({ profile: name, ...sizes[name] }));
}
writeFileSync(sizesFile, JSON.stringify(sizes, null, 2) + "\n");
