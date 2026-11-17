#!/bin/bash
set -euo pipefail
config=${1:?usage: run-server.sh SERVER_ENV}
set -a
# Private installer-generated shell assignments, supplied explicitly by launchd/process-compose.
# shellcheck source=/dev/null
source "$config"
set +a
# shellcheck source=/dev/null
source "$FAGENT_ROOT/runtime/scripts/runtime-supervise.sh"
# Pin this process to the physical release: switching runtime/current affects the next start only.
FAGENT_RELEASE=$(cd "$FAGENT_RELEASE" && pwd -P)
export PATH="$FAGENT_RELEASE/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export PLAYWRIGHT_BROWSERS_PATH="$FAGENT_RELEASE/.runtime/browsers"
export FAGENT_LAYOUT_BIN="$FAGENT_RELEASE/bin/layout-step"
# The release selects its render backend: build-release.sh writes FAGENT_RENDER_BACKEND
# (chromium by default, vello for deploy.sh) to RENDER_BACKEND, and
# the server binary also defaults to it. Older releases without the file keep the binary's
# default or server.env.
if [[ -f "$FAGENT_RELEASE/RENDER_BACKEND" ]]; then
  RENDER_BACKEND=$(head -n 1 "$FAGENT_RELEASE/RENDER_BACKEND")
  export RENDER_BACKEND
fi
# Wait on the configured database port (default 2703); the URL itself is never printed.
pg_port=${DATABASE_URL##*@}
pg_port=${pg_port%%/*}
pg_port=${pg_port##*:}
[[ "$pg_port" =~ ^[0-9]+$ ]] || pg_port=2703
for ((attempt=0; attempt<30; attempt++)); do
  /opt/homebrew/opt/postgresql@16/bin/pg_isready -h 127.0.0.1 -p "$pg_port" -q && break
  sleep 1
done
cd "$FAGENT_RELEASE"
# With PostgreSQL the data root only holds <slug>/inbox directories for asset path imports and
# batch files; MCP reports each as inboxDir. Nothing else is stored there.
supervise INT "$FAGENT_RELEASE/bin/fagent-server" \
  --workspace "$FAGENT_RELEASE" --data-root "$FAGENT_ROOT/data" \
  --host 127.0.0.1 --port "$FAGENT_PORT" \
  --public-viewer-port "$FAGENT_PUBLIC_PORT" --public-viewer-origin "$FAGENT_PUBLIC_ORIGIN"
