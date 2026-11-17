#!/bin/bash
set -euo pipefail
# Only the configuration file may select the data directory.
unset FAGENT_PGDATA
# Private installer-generated shell assignments, supplied explicitly by launchd.
# shellcheck source=/dev/null
source "${1:?usage: run-postgres.sh NATIVE_CONF}"
# shellcheck source=/dev/null
source "$FAGENT_ROOT/runtime/scripts/runtime-supervise.sh"
export LC_ALL=C LANG=C
# FAGENT_PGDATA lets the root run a cluster kept elsewhere (default <root>/postgres).
# PostgreSQL SIGINT is a fast, clean shutdown (disconnects clients and checkpoints).
supervise INT "$PG_BIN/postgres" -D "${FAGENT_PGDATA:-$FAGENT_ROOT/postgres}"
