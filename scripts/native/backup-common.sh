#!/bin/bash
# Shared private configuration and PostgreSQL command construction.
set -euo pipefail
umask 077

backup_die() { printf 'FAgent backup: %s\n' "$*" >&2; exit 1; }

backup_private_file() {
  local file=$1 mode owner
  [[ -f "$file" && ! -L "$file" ]] || backup_die "Expected a regular private config file"
  mode=$(stat -f '%Lp' "$file" 2>/dev/null) || mode=$(stat -c '%a' "$file")
  owner=$(stat -f '%u' "$file" 2>/dev/null) || owner=$(stat -c '%u' "$file")
  [[ "$owner" == "$(id -u)" && $((8#$mode & 077)) == 0 ]] ||
    backup_die "Config and passfiles must belong to this user and have mode600"
}

backup_load_config() {
  [[ $# == 1 ]] || backup_die "A private native.conf path is required"
  BACKUP_CONFIG=$1
  backup_private_file "$BACKUP_CONFIG"
  # Optional keys come only from the file, never from the caller's environment.
  unset FAGENT_PGDATA FAGENT_DATABASE_SCHEMA
  # This is an explicitly trusted, current-user-owned shell configuration.
  # shellcheck disable=SC1090
  source "$BACKUP_CONFIG"
  : "${FAGENT_ROOT:?Missing FAGENT_ROOT}" "${PGPASSFILE:?Missing PGPASSFILE}"
  PG_BIN=${PG_BIN:-/opt/homebrew/opt/postgresql@16/bin}
  PGHOST=${PGHOST:-127.0.0.1}
  PGPORT=${PGPORT:-2703}
  PGDATABASE=${PGDATABASE:-fagent}
  PGUSER=${PGUSER:-fagent}
  PGADMIN_HOST=${PGADMIN_HOST:-$FAGENT_ROOT/run}
  PGADMIN_USER=${PGADMIN_USER:-$(id -un)}
  PGADMIN_DATABASE=${PGADMIN_DATABASE:-postgres}
  BACKUP_DIR=${BACKUP_DIR:-$FAGENT_ROOT/backups}
  BACKUP_KEEP=${BACKUP_KEEP:-14}
  # Cluster data directory (default <root>/postgres) and the runtime tables' schema.
  FAGENT_PGDATA=${FAGENT_PGDATA:-$FAGENT_ROOT/postgres}
  FAGENT_DATABASE_SCHEMA=${FAGENT_DATABASE_SCHEMA:-fagent_runtime_v1}
  [[ "$FAGENT_ROOT" == /* && "$BACKUP_DIR" == /* && "$PG_BIN" == /* && "$FAGENT_PGDATA" == /* ]] ||
    backup_die "Install, backup and PostgreSQL paths must be absolute"
  [[ "$FAGENT_ROOT$BACKUP_DIR" != *$'\n'* ]] || backup_die "Paths cannot contain newlines"
  [[ "$PGPORT" =~ ^[0-9]+$ && "$BACKUP_KEEP" =~ ^[0-9]+$ && "$BACKUP_KEEP" -ge 1 ]] ||
    backup_die "Invalid port or retention count"
  # Same rule as the server (fagent_storage::DatabaseSchema): safe to place in SQL text.
  [[ "$FAGENT_DATABASE_SCHEMA" =~ ^[a-z_][a-z0-9_]{0,62}$ && "$FAGENT_DATABASE_SCHEMA" != pg_* &&
    "$FAGENT_DATABASE_SCHEMA" != information_schema ]] || backup_die "Invalid FAGENT_DATABASE_SCHEMA"
  backup_private_file "$PGPASSFILE"
  if [[ -n "${PGADMIN_PASSFILE:-}" ]]; then backup_private_file "$PGADMIN_PASSFILE"; fi
  unset PGPASSWORD DATABASE_URL PGSERVICE PGSERVICEFILE
  export PGPASSFILE PGCONNECT_TIMEOUT=10
  for tool in psql pg_dump pg_restore createdb dropdb; do
    [[ -x "$PG_BIN/$tool" ]] || backup_die "PostgreSQL16 tool missing: $tool"
  done
}

backup_app() {
  local tool=$1; shift
  "$PG_BIN/$tool" --no-password --host="$PGHOST" --port="$PGPORT" --username="$PGUSER" "$@"
}

backup_admin() {
  local tool=$1; shift
  PGPASSFILE="${PGADMIN_PASSFILE:-$PGPASSFILE}" "$PG_BIN/$tool" --no-password \
    --host="$PGADMIN_HOST" --port="$PGPORT" --username="$PGADMIN_USER" "$@"
}

backup_psql() {
  backup_admin psql --no-psqlrc --set=ON_ERROR_STOP=1 --quiet --tuples-only --no-align "$@"
}

backup_sha() { shasum -a 256 "$@"; }
