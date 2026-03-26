#!/usr/bin/env bash
set -euo pipefail

if [[ -n "${MISE_BIN:-}" ]]; then
  MISE_BIN="$MISE_BIN"
elif command -v mise >/dev/null 2>&1; then
  MISE_BIN="$(command -v mise)"
elif [[ -x "${HOME}/.local/bin/mise" ]]; then
  MISE_BIN="${HOME}/.local/bin/mise"
else
  echo "Unable to locate mise. Set MISE_BIN or ensure mise is on PATH." >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOCAL_ROOT="${OCEANS_POSTGRES_ROOT_DIR:-$ROOT_DIR/.local/postgres}"
DATA_DIR="${OCEANS_POSTGRES_DATA_DIR:-$LOCAL_ROOT/data}"
LOG_DIR="${OCEANS_POSTGRES_LOG_DIR:-$LOCAL_ROOT/logs}"
SOCKET_DIR="${OCEANS_POSTGRES_SOCKET_DIR:-$LOCAL_ROOT/run}"
HOST="${OCEANS_POSTGRES_HOST:-127.0.0.1}"
PORT="${OCEANS_POSTGRES_PORT:-5432}"
DB_NAME="${OCEANS_POSTGRES_DB:-oceans_llm}"
DB_USER="${OCEANS_POSTGRES_USER:-oceans}"
DB_PASSWORD="${OCEANS_POSTGRES_PASSWORD:-oceans}"
POSTGRES_URL="postgres://${DB_USER}:${DB_PASSWORD}@${HOST}:${PORT}/${DB_NAME}"

mkdir -p "$LOG_DIR" "$SOCKET_DIR"

run_mise() {
  "$MISE_BIN" exec -- "$@"
}

run_ready_check() {
  run_mise pg_isready \
    --host="$HOST" \
    --port="$PORT" \
    --dbname="$DB_NAME" \
    --username="$DB_USER"
}

initialize_cluster() {
  if [[ -f "$DATA_DIR/PG_VERSION" ]]; then
    return
  fi

  mkdir -p "$DATA_DIR"
  local pw_file
  pw_file="$(mktemp "${TMPDIR:-/tmp}/oceans-postgres-password.XXXXXX")"
  trap 'rm -f "$pw_file"' RETURN
  printf '%s\n' "$DB_PASSWORD" >"$pw_file"

  run_mise initdb \
    -D "$DATA_DIR" \
    --username="$DB_USER" \
    --pwfile="$pw_file" \
    --auth-local=trust \
    --auth-host=scram-sha-256

  rm -f "$pw_file"
  trap - RETURN

  cat >>"$DATA_DIR/postgresql.conf" <<EOF
listen_addresses = '${HOST}'
port = ${PORT}
unix_socket_directories = '${SOCKET_DIR}'
EOF

  run_mise pg_ctl \
    -D "$DATA_DIR" \
    -l "$LOG_DIR/bootstrap.log" \
    -o "-h ${HOST} -p ${PORT} -k ${SOCKET_DIR}" \
    -w start

  ensure_database

  run_mise pg_ctl -D "$DATA_DIR" -w stop -m fast
}

ensure_database() {
  local exists
  exists="$(
    run_mise psql \
      --host="$SOCKET_DIR" \
      --port="$PORT" \
      --username="$DB_USER" \
      --dbname=postgres \
      --tuples-only \
      --no-align \
      --command="SELECT 1 FROM pg_database WHERE datname = '${DB_NAME}'" | tr -d '[:space:]'
  )"
  if [[ "$exists" != "1" ]]; then
    run_mise createdb \
      --host="$SOCKET_DIR" \
      --port="$PORT" \
      --username="$DB_USER" \
      "$DB_NAME"
  fi
}

run_postgres() {
  initialize_cluster
  echo "Starting PostgreSQL at $POSTGRES_URL"
  exec "$MISE_BIN" exec -- postgres \
    -D "$DATA_DIR" \
    -h "$HOST" \
    -p "$PORT" \
    -k "$SOCKET_DIR"
}

reset_cluster() {
  if [[ -f "$DATA_DIR/PG_VERSION" ]]; then
    run_mise pg_ctl -D "$DATA_DIR" -m fast stop >/dev/null 2>&1 || true
  fi
  rm -rf "$LOCAL_ROOT"
}

print_env_exports() {
  cat <<EOF
export OCEANS_POSTGRES_HOST="${HOST}"
export OCEANS_POSTGRES_PORT="${PORT}"
export OCEANS_POSTGRES_DB="${DB_NAME}"
export OCEANS_POSTGRES_USER="${DB_USER}"
export OCEANS_POSTGRES_PASSWORD="${DB_PASSWORD}"
export POSTGRES_URL="${POSTGRES_URL}"
export TEST_POSTGRES_URL="${POSTGRES_URL}"
EOF
}

case "${1:-run}" in
run)
  run_postgres
  ;;
ready)
  run_ready_check
  ;;
ensure-db)
  ensure_database
  ;;
reset)
  reset_cluster
  ;;
print-env)
  print_env_exports
  ;;
*)
  echo "Unknown command: ${1:-}" >&2
  echo "Usage: $0 [run|ready|ensure-db|reset|print-env]" >&2
  exit 1
  ;;
esac
