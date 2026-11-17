import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { readdir, readFile, writeFile } from "node:fs/promises";
import { join, relative } from "node:path";
import { hash } from "./scene.mjs";
const exec = promisify(execFile);
export async function sourceHashes(root) {
  const result = {};
  async function walk(dir) {
    for (const entry of await readdir(dir, { withFileTypes: true })) {
      if (["target", "out", "node_modules", ".git", "dist"].includes(entry.name)) continue;
      const path = join(dir, entry.name);
      if (entry.isDirectory()) await walk(path);
      else if (/\.(rs|mjs|ts|toml)$/.test(entry.name)) result[relative(root, path)] = hash(await readFile(path));
    }
  }
  for (const dir of ["crates/paint-core/src", "crates/paint-vello/src", "crates/text/src", "renderer", "spikes/native-paint-parity"]) {
    // Do not traverse outputs/dependencies. Only these source directories are visited.
    if (dir.endsWith("native-paint-parity")) {
      for (const entry of await readdir(join(root, dir))) if (/\.(mjs|md)$/.test(entry)) {
        result[`${dir}/${entry}`] = hash(await readFile(join(root, dir, entry)));
      }
    } else if (dir === "renderer") {
      for (const entry of await readdir(join(root, dir))) if (entry.endsWith(".ts")) result[`${dir}/${entry}`] = hash(await readFile(join(root, dir, entry)));
    } else await walk(join(root, dir));
  }
  return result;
}
// The server's `vello` export (paint-vello + vello_cpu + shared paint-core), same adapter protocol.
export async function buildVello(root, out) {
  const args = ["-p", "fagent-paint-vello", "--features", "cli", "--offline"];
  try {
    const build = await exec("cargo", ["build", "--release", "--bin", "paint-vello-export", ...args], { cwd: root, maxBuffer: 8 << 20 });
    await writeFile(join(out, "vello-build.log"), build.stdout + build.stderr);
  } catch (error) {
    await writeFile(join(out, "vello-build.log"), (error.stdout ?? "") + (error.stderr ?? ""));
    throw error;
  }
  const tree = (await exec("cargo", ["tree", ...args, "-e", "normal", "--prefix", "none", "-f", "{p}\t{l}"], { cwd: root, maxBuffer: 8 << 20 })).stdout;
  const deps = [...new Set(tree.split("\n").filter(Boolean).map(line => line.replace(" (*)", "")))].sort().map(line => {
    const [pkg, license] = line.split("\t"), [name, version] = pkg.split(" ");
    return { name, version: version?.replace(/^v/, ""), license: license || null, source: pkg.includes("(/") ? "path" : "registry" };
  });
  return { binary: join(root, "target/release/paint-vello-export"), deps, rustc: (await exec("rustc", ["--version"])).stdout.trim() };
}
export async function nativeCapture(binary, out, request, png) {
  const result = await exec(binary, [out, request, png], { maxBuffer: 2 << 20 });
  return JSON.parse(result.stdout);
}
