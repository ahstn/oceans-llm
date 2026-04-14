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
WEB_DIR="$ROOT_DIR/crates/admin-ui/web"
RUNTIME_DIR="$(mktemp -d "${TMPDIR:-/tmp}/oceans-e2e.XXXXXX")"
CONFIG_PATH="$RUNTIME_DIR/gateway.e2e.yaml"
DB_PATH="$RUNTIME_DIR/gateway.e2e.db"
MOCK_LOG="$RUNTIME_DIR/mock-upstream.log"
UI_LOG="$RUNTIME_DIR/admin-ui.log"
GATEWAY_LOG="$RUNTIME_DIR/gateway.log"

E2E_GATEWAY_PORT="${E2E_GATEWAY_PORT:-38080}"
E2E_UI_PORT="${E2E_UI_PORT:-33001}"
E2E_UPSTREAM_PORT="${E2E_UPSTREAM_PORT:-38081}"
E2E_BASE_URL="${E2E_BASE_URL:-http://127.0.0.1:${E2E_GATEWAY_PORT}}"
E2E_GATEWAY_API_KEY="${E2E_GATEWAY_API_KEY:-gwk_e2e.secret-value}"
E2E_ADMIN_EMAIL="${E2E_ADMIN_EMAIL:-admin@local}"
E2E_ADMIN_PASSWORD="${E2E_ADMIN_PASSWORD:-admin}"
E2E_ADMIN_NEW_PASSWORD="${E2E_ADMIN_NEW_PASSWORD:-s3cur3-passw0rd}"

export E2E_GATEWAY_PORT
export E2E_UI_PORT
export E2E_UPSTREAM_PORT
export E2E_BASE_URL
export E2E_GATEWAY_API_KEY
export E2E_ADMIN_EMAIL
export E2E_ADMIN_PASSWORD
export E2E_ADMIN_NEW_PASSWORD

cleanup() {
  for pid in "${GATEWAY_PID:-}" "${UI_PID:-}" "${MOCK_PID:-}"; do
    if [[ -n "${pid:-}" ]]; then
      kill "$pid" >/dev/null 2>&1 || true
      wait "$pid" >/dev/null 2>&1 || true
    fi
  done

  rm -rf "$RUNTIME_DIR"
}

trap cleanup EXIT INT TERM

cat >"$CONFIG_PATH" <<EOF
server:
  bind: "127.0.0.1:${E2E_GATEWAY_PORT}"
  log_format: "pretty"

database:
  path: "${DB_PATH}"

auth:
  seed_api_keys:
    - name: "E2E Contract Key"
      value: env.E2E_GATEWAY_API_KEY
      allowed_models: ["fast"]
  bootstrap_admin:
    enabled: true
    email: "${E2E_ADMIN_EMAIL}"
    password: "literal.${E2E_ADMIN_PASSWORD}"
    require_password_change: true

providers:
  - id: openai-e2e
    type: openai_compat
    base_url: http://127.0.0.1:${E2E_UPSTREAM_PORT}/v1
    pricing_provider_id: openai
    auth:
      kind: bearer
      token: literal.upstream-e2e-token

models:
  - id: fast
    description: E2E test route
    routes:
      - provider: openai-e2e
        upstream_model: gpt-4o-mini
  - id: reasoning
    description: E2E reasoning route
    routes:
      - provider: openai-e2e
        upstream_model: gpt-4.1
EOF

(
  cd "$ROOT_DIR"
  "$MISE_BIN" exec -- node scripts/mock-openai-upstream.mjs
) >"$MOCK_LOG" 2>&1 &
MOCK_PID=$!

(
  cd "$WEB_DIR"
  "$MISE_BIN" exec -- bun run build
)

(
  cd "$WEB_DIR"
  PORT="$E2E_UI_PORT" "$MISE_BIN" exec -- bun run start
) >"$UI_LOG" 2>&1 &
UI_PID=$!

(
  cd "$ROOT_DIR"
  ADMIN_UI_UPSTREAM="http://127.0.0.1:${E2E_UI_PORT}" \
  GATEWAY_CONFIG="$CONFIG_PATH" \
  GATEWAY_IDENTITY_TOKEN_SECRET="local-dev-identity-secret" \
    "$MISE_BIN" exec -- cargo run -p gateway --bin gateway
) >"$GATEWAY_LOG" 2>&1 &
GATEWAY_PID=$!

READY_URL="${E2E_BASE_URL}/readyz"
for _ in $(seq 1 300); do
  if curl --silent --fail "$READY_URL" >/dev/null 2>&1; then
    echo "E2E stack ready"
    echo "Gateway: $E2E_BASE_URL"
    echo "UI upstream: http://127.0.0.1:${E2E_UI_PORT}"
    echo "Mock upstream: http://127.0.0.1:${E2E_UPSTREAM_PORT}"
    while true; do
      for pid in "$MOCK_PID" "$UI_PID" "$GATEWAY_PID"; do
        if ! kill -0 "$pid" >/dev/null 2>&1; then
          status=0
          wait "$pid" || status=$?
          exit "$status"
        fi
      done
      sleep 1
    done
  fi

  for pair in \
    "mock:$MOCK_PID:$MOCK_LOG" \
    "ui:$UI_PID:$UI_LOG" \
    "gateway:$GATEWAY_PID:$GATEWAY_LOG"; do
    IFS=':' read -r name pid log <<<"$pair"
    if ! kill -0 "$pid" >/dev/null 2>&1; then
      echo "E2E stack failed while starting: $name exited" >&2
      if [[ -f "$log" ]]; then
        cat "$log" >&2
      fi
      exit 1
    fi
  done

  sleep 1
done

echo "Timed out waiting for E2E stack readiness at $READY_URL" >&2
for log in "$MOCK_LOG" "$UI_LOG" "$GATEWAY_LOG"; do
  if [[ -f "$log" ]]; then
    echo "===== $log =====" >&2
    cat "$log" >&2
  fi
done
exit 1
