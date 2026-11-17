import { spawn, spawnSync } from "node:child_process";
import { access } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const workspace = fileURLToPath(new URL("../", import.meta.url));
const [action, profile, ...args] = process.argv.slice(2);
const target = path.resolve(
  workspace,
  process.env.CARGO_TARGET_DIR ?? "target",
);
const suffix = process.platform === "win32" ? ".exe" : "";
const binary = (name) => path.join(target, profile, `${name}${suffix}`);
const buildCommand = profile === "release" ? "build:release" : "build";

// Build-time export backend (FAGENT_RENDER_BACKEND): baked into the server binary as its
// default and deciding what is built. chromium (default) also builds the renderer bundle and
// layout-step for the Node worker; vello builds neither, so the server needs no Node/Chromium.
function buildBackend() {
  const value = process.env.FAGENT_RENDER_BACKEND || "chromium";
  if (!["chromium", "vello"].includes(value))
    throw new Error("FAGENT_RENDER_BACKEND must be chromium or vello");
  return value;
}

// `fagent-server --help` ends with "This build defaults to RENDER_BACKEND=<name>."
function bakedBackend(server) {
  const help = spawnSync(server, ["--help"], { encoding: "utf8" }).stdout ?? "";
  return /defaults to RENDER_BACKEND=(\w+)/.exec(help)?.[1] ?? "chromium";
}

// Run from the repository even when invoked by npm --prefix from another directory.
// npm's CLI runs via Node, avoiding shell quoting and Windows .cmd differences.
async function run(command, argv, env = process.env) {
  const child = spawn(command, argv, { cwd: workspace, env, stdio: "inherit" });
  const interrupt = () => child.kill("SIGINT");
  process.on("SIGINT", interrupt);
  process.on("SIGTERM", interrupt);
  try {
    await new Promise((resolve, reject) => {
      child.once("error", reject);
      child.once("close", (code, signal) => {
        if (code === 0) resolve();
        else {
          const error = new Error(
            `${path.basename(command)} ${signal ?? `exited ${code}`}`,
          );
          error.exitCode = code ?? 1;
          reject(error);
        }
      });
    });
  } finally {
    process.off("SIGINT", interrupt);
    process.off("SIGTERM", interrupt);
  }
}

async function requireArtifact(file) {
  try {
    await access(file);
  } catch {
    throw new Error(
      `Missing built artifact: ${file}\nRun npm run ${buildCommand} first.`,
    );
  }
}

try {
  if (!["debug", "release"].includes(profile))
    throw new Error("Unknown app profile");
  if (action === "build") {
    const npmCli = process.env.npm_execpath;
    if (!npmCli) throw new Error("Use npm run build or npm run build:release.");
    const backend = buildBackend();
    const chromium = backend === "chromium";
    if (chromium)
      await run(process.execPath, [npmCli, "run", "build:renderer"]);
    await run(
      "cargo",
      [
        "build",
        "--locked",
        ...(chromium ? ["-p", "fagent-layout"] : []),
        "-p",
        "fagent-server",
        ...(profile === "release" ? ["--release"] : []),
        ...args,
      ],
      { ...process.env, FAGENT_RENDER_BACKEND: backend },
    );
    await run(process.execPath, [npmCli, "--prefix", "web", "run", "build"]);
  } else if (action === "run") {
    await requireArtifact(binary("fagent-server"));
    // The binary's own default is baked in at build time; RENDER_BACKEND overrides it.
    const chromium =
      (process.env.RENDER_BACKEND ?? bakedBackend(binary("fagent-server"))) ===
      "chromium";
    for (const artifact of [
      ...(chromium
        ? [
            binary("layout-step"),
            path.join(workspace, "renderer/dist/index.js"),
          ]
        : []),
      path.join(workspace, "web/dist/index.html"),
    ])
      await requireArtifact(artifact);
    await run(binary("fagent-server"), ["--workspace", workspace, ...args], {
      ...process.env,
      FAGENT_LAYOUT_BIN: binary("layout-step"),
      PLAYWRIGHT_BROWSERS_PATH:
        process.env.PLAYWRIGHT_BROWSERS_PATH ??
        path.join(workspace, ".runtime/browsers"),
    });
  } else throw new Error("Unknown app action");
} catch (error) {
  console.error(error.message);
  process.exitCode = error.exitCode ?? 1;
}
