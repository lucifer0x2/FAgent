#!/bin/sh
# Viewer engine (Task 30): release wasm32 with SIMD (simd128 is required: without it the
# Vello encoder is ~2.6x slower) and wasm-bindgen ESM glue. Shared by the local Node
# wrapper and the Rust-only Docker stage; scripts/package-engine-wasm.mjs names the
# output by content hash and writes compressed sidecars.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
output=${1:-"$root/target/engine-wasm/pkg"}
target_dir=${2:-"$root/target/engine-wasm"}
bindgen=${WASM_BINDGEN:-wasm-bindgen}

cd "$root"
# Speed profile from the Task 29 measurements (O3, thin LTO); panic=abort like scene-wasm.
CARGO_PROFILE_RELEASE_OPT_LEVEL=3 \
CARGO_PROFILE_RELEASE_LTO=thin \
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
CARGO_PROFILE_RELEASE_PANIC=abort \
CARGO_PROFILE_RELEASE_DEBUG=false \
RUSTFLAGS="-C target-feature=+simd128" \
    cargo build --locked --release -p fagent-engine-wasm \
        --target wasm32-unknown-unknown --target-dir "$target_dir"
rm -rf "$output"
mkdir -p "$output"
"$bindgen" "$target_dir/wasm32-unknown-unknown/release/fagent_engine_wasm.wasm" \
    --target web --out-dir "$output" --out-name fagent_engine_wasm
rm -f "$output/package.json"
