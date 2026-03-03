#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WEB_DIR="$ROOT_DIR/crates/admin-ui/web"

GATEWAY_PORT="${PORT:-8080}"
ADMIN_UI_INTERNAL_PORT="${ADMIN_UI_INTERNAL_PORT:-3001}"
ADMIN_UI_UPSTREAM="${ADMIN_UI_UPSTREAM:-http://127.0.0.1:${ADMIN_UI_INTERNAL_PORT}}"

cleanup() {
  if [[ -n "${UI_PID:-}" ]]; then
    kill "$UI_PID" >/dev/null 2>&1 || true
  fi
}

trap cleanup EXIT INT TERM

PORT="$ADMIN_UI_INTERNAL_PORT" bun run --cwd "$WEB_DIR" start &
UI_PID=$!

ADMIN_UI_UPSTREAM="$ADMIN_UI_UPSTREAM" PORT="$GATEWAY_PORT" cargo run -p gateway
