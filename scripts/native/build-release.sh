#!/bin/bash
# Builds one complete native release from one git commit. No Docker, no patch overlays.
set -euo pipefail
umask 077
HERE=$(cd "$(dirname "$0")" && pwd -P)
# shellcheck source=scripts/native/deploy-common.sh
source "$HERE/deploy-common.sh"

usage() {
  cat <<'EOF'
Usage: build-release.sh <git-ref> <output-dir>

Resolves <git-ref> to a full commit, archives that exact tree into a private source
directory and builds natively: scene WASM, the Vello engine WASM, viewer and release server,
plus for chromium the renderer bundle, layout-step, worker and Playwright. Copies Node (only
verify-runtime.mjs uses it), then writes RENDER_BACKEND, REVISION, PROVENANCE and SHA256SUMS.
The server binary defaults to the build's FAGENT_RENDER_BACKEND and run-server.sh exports the
release's RENDER_BACKEND file. A vello release has no worker, renderer bundle, layout-step,
Playwright or browser: its server needs no Node or Chromium.
<output-dir> must not exist; it appears only after a complete build.

Environment (optional):
  CARGO_TARGET_DIR     Cargo cache (default <repo>/.runtime/native/target)
  CARGO_BUILD_JOBS     Parallel Cargo jobs
  WASM_BINDGEN         wasm-bindgen matching Cargo.lock (default: .runtime/native/tools,
                       target/scene-wasm/tools, then PATH)
  FAGENT_NODE      Node binary matching the Dockerfile pin (default nvm Node)
  FAGENT_RENDER_BACKEND
                       chromium (default) or vello, baked into the server as its default.
                       chromium also builds/copies the Node worker runtime and the Playwright
                       browser from FAGENT_BROWSERS (default <repo>/.runtime/browsers).
                       deploy.sh builds vello.
Builds run offline (cargo --offline, npm ci --offline); dependencies must be cached.
EOF
}

[[ "${1:-}" != --help && "${1:-}" != -h ]] || { usage; exit 0; }
[[ $# == 2 ]] || { usage >&2; exit 2; }
ref=$1
output=$2
[[ "$output" == /* ]] || output="$PWD/$output"
[[ ! -e "$output" ]] || die "Output already exists: $output"
parent=$(dirname "$output")
[[ -d "$parent" ]] || die "Output parent directory does not exist: $parent"
sha=$(git -C "$NATIVE_REPO" rev-parse --verify --quiet "$ref^{commit}") || die "Unknown git ref: $ref"
backend=${FAGENT_RENDER_BACKEND:-chromium}
[[ "$backend" == vello || "$backend" == chromium ]] ||
  die "FAGENT_RENDER_BACKEND must be chromium or vello"
# The server reads it at compile time (its default render backend).
export FAGENT_RENDER_BACKEND=$backend
cache="$NATIVE_REPO/.runtime/native"
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$cache/target}
export CARGO_NET_OFFLINE=true
started=$SECONDS
src="$cache/source/$sha"
staging=''
cleanup() {
  local status=$?
  trap - EXIT
  [[ -z "$staging" ]] || rm -rf "$staging"
  ((status == 0)) || printf 'error: build of %s failed; nothing was written to %s\n' "$sha" "$output" >&2
  exit "$status"
}
trap cleanup EXIT

say "Archiving $sha ($ref)"
mkdir -p "$cache/source"
rm -rf "$src"
mkdir -p "$src"
git -C "$NATIVE_REPO" archive "$sha" | tar -x -C "$src"

say "Checking toolchain against the Dockerfile pins"
node=${FAGENT_NODE:-$HOME/.nvm/versions/node/v24.21.0/bin/node}
node_pin=$(sed -n 's/^ARG NODE_IMAGE=node:\([0-9.]*\)-.*/\1/p' "$src/Dockerfile")
rust_pin=$(sed -n 's/^ARG RUST_IMAGE=rust:\([0-9.]*\)-.*/\1/p' "$src/Dockerfile")
[[ -x "$node" && "$("$node" --version)" == "v$node_pin" ]] || die "Node v$node_pin required (set FAGENT_NODE)"
node_dir=$(dirname "$node")
export PATH="$node_dir:$PATH"
rustc_version=$(rustc --version)
[[ "$rustc_version" == "rustc $rust_pin "* ]] || warn "Dockerfile pins Rust $rust_pin; building with $rustc_version"
rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown ||
  die "Rust target wasm32-unknown-unknown is not installed"
bindgen_pin=$(awk '$0 == "name = \"wasm-bindgen\"" { getline; gsub(/version = |"/, ""); print }' "$src/Cargo.lock")
bindgen=${WASM_BINDGEN:-}
# Project-local installs (this cache, or npm run build:scene-wasm -- --install-tools), then PATH.
for candidate in "$cache/tools/bin/wasm-bindgen" "$NATIVE_REPO/target/scene-wasm/tools/bin/wasm-bindgen" \
  "$(command -v wasm-bindgen || true)"; do
  [[ -n "$bindgen" || -z "$candidate" || ! -x "$candidate" ]] || bindgen=$candidate
done
[[ -n "$bindgen" && "$("$bindgen" --version 2>/dev/null)" == "wasm-bindgen $bindgen_pin" ]] ||
  die "wasm-bindgen $bindgen_pin required: set WASM_BINDGEN or install it project-locally with
  cargo install wasm-bindgen-cli --version $bindgen_pin --locked --root .runtime/native/tools"
browsers=${FAGENT_BROWSERS:-$NATIVE_REPO/.runtime/browsers}

say "Building scene WASM"
(cd "$src" && WASM_BINDGEN="$bindgen" sh scripts/build-scene-wasm.sh "$src/web/public/scene-wasm" "$CARGO_TARGET_DIR/scene-wasm")
(cd "$src" && node scripts/compress-scene-wasm.mjs web/public/scene-wasm)

say "Building the Vello viewer engine WASM (as the Dockerfile engine-wasm stage)"
(cd "$src" && WASM_BINDGEN="$bindgen" sh scripts/build-engine-wasm.sh "$CARGO_TARGET_DIR/engine-wasm/pkg" "$CARGO_TARGET_DIR/engine-wasm")
(cd "$src" && node scripts/package-engine-wasm.mjs "$CARGO_TARGET_DIR/engine-wasm/pkg" web/public/engine-wasm >/dev/null)

say "Installing locked npm dependencies from the local cache"
(cd "$src" && npm ci --offline --no-audit --no-fund && npm --prefix web ci --offline --no-audit --no-fund)

if [[ "$backend" == chromium ]]; then
  say "Building renderer bundle and viewer"
  (cd "$src" && npm run build:renderer && npm --prefix web run build)
  say "Building layout-step (debug, as in the Dockerfile) and the release server (default chromium)"
  cargo build --offline --locked --manifest-path "$src/Cargo.toml" -p fagent-layout --bin layout-step
else
  say "Building the viewer and the release server (default vello; no renderer bundle or layout-step)"
  (cd "$src" && npm --prefix web run build)
fi
cargo build --offline --locked --release --manifest-path "$src/Cargo.toml" -p fagent-server --bin fagent-server

chromium_revision=$(node -p 'require(process.argv[1]).browsers.find((b) => b.name === "chromium-headless-shell").revision' \
  "$src/node_modules/playwright-core/browsers.json")
[[ "$backend" != chromium || -d "$browsers/chromium_headless_shell-$chromium_revision" ]] ||
  die "Playwright browser chromium_headless_shell-$chromium_revision not found under $browsers"
db_schema=$(parse_db_schema < "$src/$DB_SCHEMA_SOURCE")
scene_schema=$(parse_scene_schema < "$src/$SCENE_SCHEMA_SOURCE")
[[ "$db_schema" =~ ^[0-9]+$ && "$scene_schema" =~ ^[0-9]+$ ]] ||
  die "Cannot determine the supported schema versions from $DB_SCHEMA_SOURCE and $SCENE_SCHEMA_SOURCE"

say "Assembling release"
staging=$(mktemp -d "$parent/.build-${sha:0:12}.XXXXXX")
mkdir -p "$staging/bin" "$staging/web"
cp "$CARGO_TARGET_DIR/release/fagent-server" "$staging/bin/"
# Node runs verify-runtime.mjs (deploy acceptance) only; a vello server never starts it.
cp "$node" "$staging/bin/node"
cp -R "$src/web/dist" "$staging/web/dist"
cp "$src/Cargo.lock" "$staging/"
printf '%s\n' "$backend" > "$staging/RENDER_BACKEND"
packaged=(bin web Cargo.lock RENDER_BACKEND)
if [[ "$backend" == chromium ]]; then
  # The Node worker runtime: layout-step, worker, renderer bundle and font, Playwright, browser.
  mkdir -p "$staging/renderer" "$staging/node_modules" "$staging/.runtime"
  cp "$CARGO_TARGET_DIR/debug/layout-step" "$staging/bin/"
  cp -R "$src/worker" "$staging/worker"
  cp -R "$src/renderer/dist" "$src/renderer/fonts" "$staging/renderer/"
  cp -R "$src/node_modules/playwright" "$src/node_modules/playwright-core" "$staging/node_modules/"
  cp "$src/package.json" "$src/package-lock.json" "$staging/"
  packaged+=(worker renderer node_modules package.json package-lock.json)
  # APFS clones keep per-release browser copies cheap; fall back to a plain copy elsewhere.
  cp -cR "$browsers" "$staging/.runtime/browsers" 2>/dev/null || {
    rm -rf "$staging/.runtime/browsers"
    cp -R "$browsers" "$staging/.runtime/browsers"
  }
fi
printf '%s\n' "$sha" > "$staging/REVISION"
{
  printf 'commit=%s\nref=%s\nbuilt_utc=%s\nbuild_seconds=%s\n' "$sha" "$ref" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$((SECONDS - started))"
  printf 'builder=scripts/native/build-release.sh\nsource=git-archive\n'
  printf 'database_schema_version=%s\nscene_schema_version=%s\n' "$db_schema" "$scene_schema"
  printf 'rustc=%s\ncargo=%s\n' "$rustc_version" "$(cargo --version)"
  printf 'node=%s\nnpm=%s\nwasm_bindgen=%s\n' "$(node --version)" "$(npm --version)" "$bindgen_pin"
  if [[ "$backend" == chromium ]]; then
    printf 'playwright=%s\nchromium_headless_shell_revision=%s\n' \
      "$(node -p 'require(process.argv[1]).version' "$src/node_modules/playwright/package.json")" "$chromium_revision"
    printf 'layout_step_profile=debug\n'
  fi
  printf 'server_profile=release\nrender_backend=%s\n' "$backend"
} > "$staging/PROVENANCE"
(cd "$staging" && find "${packaged[@]}" -type f -print0 |
  LC_ALL=C sort -z | xargs -0 shasum -a 256 > SHA256SUMS)
mv "$staging" "$output"
staging=''
# The Cargo target directory is the reusable cache; a failed build keeps its source for diagnosis.
rm -rf "$src"
say "Release $sha built in $((SECONDS - started))s: $output"
