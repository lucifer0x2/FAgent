#!/usr/bin/env node
// Viewer engine module (renderer=vello): build, then hash-named copy into web/public.
// Pinned wasm-bindgen glue must match the crate version in crates/engine-wasm.
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { packageEngineWasm } from "./package-engine-wasm.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const version = "wasm-bindgen 0.2.129";
const targetDir = join(root, "target", "engine-wasm");
const bindgenOut = join(targetDir, "pkg");
const output = join(root, "web", "public", "engine-wasm");
for (const argument of process.argv.slice(2))
  throw new Error(`Unknown argument: ${argument}`);

const candidates = [
  process.env.WASM_BINDGEN,
  join(root, "target", "wasm-tools", "bin", "wasm-bindgen"),
  join(root, "target", "scene-wasm", "tools", "bin", "wasm-bindgen"),
  "wasm-bindgen",
].filter(Boolean);
const bindgen = candidates.find(
  (command) =>
    (command === "wasm-bindgen" || existsSync(command)) &&
    spawnSync(command, ["--version"], { encoding: "utf8" }).stdout?.trim() ===
      version,
);
if (!bindgen)
  throw new Error(
    `Requires ${version}. Set WASM_BINDGEN, or run npm run build:scene-wasm -- --install-tools.`,
  );

const result = spawnSync(
  "sh",
  [join(root, "scripts", "build-engine-wasm.sh"), bindgenOut, targetDir],
  { cwd: root, stdio: "inherit", env: { ...process.env, WASM_BINDGEN: bindgen } },
);
if (result.status !== 0) throw new Error(`engine build exited ${result.status}`);
console.log(JSON.stringify(packageEngineWasm(bindgenOut, output)));
