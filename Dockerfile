# syntax=docker/dockerfile:1
# FAgent in one image: Rust server and viewer, plus the PNG export runtime chosen by the build
# argument FAGENT_RENDER_BACKEND (baked into the server as its default backend):
#   chromium (default): layout-step, Node render worker with Playwright Chromium, renderer bundle.
#   vello: none of those; the runtime image has no Node or Chromium (vello_cpu in the server).
# Build: docker build -t fagent:local .  (or --build-arg FAGENT_RENDER_BACKEND=vello)
ARG RUST_IMAGE=rust:1.99.1-slim-trixie
ARG NODE_IMAGE=node:24.21.0-trixie-slim
# Same Debian release as RUST_IMAGE, so the release server's glibc matches.
ARG RUNTIME_IMAGE=debian:trixie-slim
ARG FAGENT_RENDER_BACKEND=chromium

FROM ${RUST_IMAGE} AS scene-wasm
WORKDIR /src
# Tool installation is independent of project sources and remains cached across edits.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/tmp/wasm-bindgen-target \
    rustup target add wasm32-unknown-unknown \
    && CARGO_TARGET_DIR=/tmp/wasm-bindgen-target cargo install wasm-bindgen-cli \
        --version 0.2.129 --locked --root /opt/wasm-bindgen
COPY Cargo.toml Cargo.lock ./
COPY crates crates
# A workspace member outside crates/ (Task 29 spike): cargo loads every member manifest.
COPY spikes/vello-gpu/wasm spikes/vello-gpu/wasm
COPY scripts/build-scene-wasm.sh scripts/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target/scene-wasm \
    WASM_BINDGEN=/opt/wasm-bindgen/bin/wasm-bindgen \
    sh scripts/build-scene-wasm.sh /out/scene-wasm /src/target/scene-wasm

# Viewer engine for renderer=vello (Task 30): same toolchain stage, SIMD release build.
FROM scene-wasm AS engine-wasm
COPY scripts/build-engine-wasm.sh scripts/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target/engine-wasm \
    WASM_BINDGEN=/opt/wasm-bindgen/bin/wasm-bindgen \
    sh scripts/build-engine-wasm.sh /out/engine-wasm /src/target/engine-wasm

FROM ${RUST_IMAGE} AS rust
ARG FAGENT_RENDER_BACKEND
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
# A workspace member outside crates/ (Task 29 spike): cargo loads every member manifest.
COPY spikes/vello-gpu/wasm spikes/vello-gpu/wasm
# fagent-storage embeds the bundled font and its licence (include_bytes!).
COPY renderer/fonts renderer/fonts
# The server reads FAGENT_RENDER_BACKEND at compile time as its default backend.
# layout-step (chromium only) stays in the dev profile: the worker (worker/pipeline.mjs) and the
# pipeline fingerprint (crates/server/src/fingerprint.rs) look for
# <workspace>/target/debug/layout-step, and it is the build the native acceptance runs verified.
# The server itself is a release build.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    case "$FAGENT_RENDER_BACKEND" in chromium|vello) ;; \
      *) echo "FAGENT_RENDER_BACKEND must be chromium or vello" >&2; exit 1 ;; esac \
    && mkdir /out \
    && if [ "$FAGENT_RENDER_BACKEND" = chromium ]; then \
         cargo build --locked -p fagent-layout --bin layout-step \
         && cp target/debug/layout-step /out/; \
       fi \
    && FAGENT_RENDER_BACKEND="$FAGENT_RENDER_BACKEND" cargo build --locked --release -p fagent-server \
    && cp target/release/fagent-server /out/

FROM ${NODE_IMAGE} AS web
ARG FAGENT_RENDER_BACKEND
WORKDIR /src
COPY package.json package-lock.json tsconfig.json ./
COPY web/package.json web/package-lock.json web/
RUN --mount=type=cache,target=/root/.npm npm ci && npm --prefix web ci
# The viewer imports renderer/ sources; only the chromium worker needs the renderer bundle.
COPY renderer renderer
COPY web web
COPY --from=scene-wasm /out/scene-wasm web/public/scene-wasm
COPY --from=engine-wasm /out/engine-wasm /tmp/engine-wasm
COPY scripts/compress-scene-wasm.mjs scripts/package-engine-wasm.mjs scripts/
RUN node scripts/package-engine-wasm.mjs /tmp/engine-wasm web/public/engine-wasm \
    && node scripts/compress-scene-wasm.mjs web/public/scene-wasm \
    && if [ "$FAGENT_RENDER_BACKEND" = chromium ]; then npm run build:renderer; fi \
    && npm --prefix web run build

FROM ${NODE_IMAGE} AS runtime-chromium
ENV PLAYWRIGHT_BROWSERS_PATH=/app/.runtime/browsers
WORKDIR /app
# The worker and the fingerprint import `playwright`; nothing else from node_modules runs here.
COPY --from=web /src/node_modules/playwright node_modules/playwright
COPY --from=web /src/node_modules/playwright-core node_modules/playwright-core
# chromium.launch({headless:true}) uses the headless shell, so the full browser is not installed.
RUN node node_modules/playwright/cli.js install --with-deps --only-shell chromium \
    && rm -rf /var/lib/apt/lists/* /tmp/*
# Lockfiles are hashed into the pipeline identity; worker, renderer bundle and font are served or
# executed from the workspace (/app).
COPY package.json package-lock.json Cargo.lock ./
COPY worker worker
COPY renderer/fonts renderer/fonts
COPY --from=web /src/renderer/dist renderer/dist
COPY --from=web /src/web/dist web/dist
COPY --from=rust /out/layout-step target/debug/layout-step
COPY --from=rust /out/fagent-server /usr/local/bin/fagent-server
RUN mkdir /data && chown node:node /data
USER node

# No Node, Playwright, worker, renderer bundle or layout-step: the server computes and paints
# in process. uid/gid 1000 as the node user of the chromium image (volume ownership matches).
FROM ${RUNTIME_IMAGE} AS runtime-vello
WORKDIR /app
RUN groupadd --gid 1000 fagent && useradd --uid 1000 --gid 1000 --no-create-home fagent \
    && mkdir /data && chown fagent:fagent /data
COPY --from=web /src/web/dist web/dist
COPY --from=rust /out/fagent-server /usr/local/bin/fagent-server
USER fagent

FROM runtime-${FAGENT_RENDER_BACKEND} AS runtime
VOLUME /data
EXPOSE 3847
# The server stops gracefully on SIGINT (Ctrl-C) only.
STOPSIGNAL SIGINT
CMD ["fagent-server", "--workspace", "/app", "--data-root", "/data", "--host", "0.0.0.0", "--port", "3847"]
