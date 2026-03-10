# Oceans LLM Gateway

Rust-first gateway workspace with an embedded `admin-ui` crate that hosts a TanStack Start control-plane UI.

## Workspace layout

- `crates/gateway`: Rust API binary (`/healthz`, `/readyz`, `/v1/*`)
- `crates/gateway-core`: shared domain types, traits, OpenAI-compatible DTOs, typed errors
- `crates/gateway-store`: libsql + PostgreSQL store implementations, migrations, seed upserts
- `crates/gateway-service`: auth, model resolution, route planning, orchestration
- `crates/gateway-providers`: reqwest provider transport scaffolding
- `crates/admin-ui`: Rust reverse proxy integration for `/admin*`
- `crates/admin-ui/web`: TanStack Start + React + shadcn-style UI implementation

## Runtime model

Single-container dual process:

1. Gateway (Rust) listens on `PORT` (default `8080`)
2. Admin UI SSR process (Bun/TanStack Start) runs on internal `3001`
3. Gateway reverse-proxies `/admin*` to `ADMIN_UI_UPSTREAM`

## Environment

- `PORT`: Gateway bind port (default `8080`)
- `GATEWAY_CONFIG`: gateway config file path (default `./gateway.yaml`, prod helper uses `./gateway.prod.yaml`)
- `GATEWAY_RUN_MIGRATIONS`: control `gateway serve --run-migrations` (default `true`)
- `GATEWAY_BOOTSTRAP_ADMIN`: control `gateway serve --bootstrap-admin` (default `true`)
- `GATEWAY_SEED_CONFIG`: control `gateway serve --seed-config` (default `true`)
- `POSTGRES_URL`: PostgreSQL connection string used by production-shaped configs (for example `postgres://oceans:oceans@localhost:5432/oceans_llm`)
- `ADMIN_UI_BASE_PATH`: UI mount path (default `/admin`)
- `ADMIN_UI_UPSTREAM`: SSR upstream URL (default `http://localhost:3001`)
- `ADMIN_UI_CONNECT_TIMEOUT_MS`: Proxy connect timeout (default `750`)
- `ADMIN_UI_REQUEST_TIMEOUT_MS`: Proxy request timeout (default `10000`)
- `ADMIN_UI_INTERNAL_PORT`: Internal Bun SSR port used by helper scripts (default `3001`)

## Gateway config

`gateway` now exposes explicit operational commands:

- `gateway serve`: normal runtime startup
- `gateway migrate --status|--check|--apply`: inspect or apply database migrations
- `gateway bootstrap-admin`: ensure the configured bootstrap admin exists
- `gateway seed-config`: seed providers/models/API keys without starting HTTP

The repo exposes matching `mise` tasks:

- `mise run gateway-serve`
- `mise run gateway-migrate`
- `mise run gateway-bootstrap-admin`
- `mise run gateway-seed-config`

`gateway-serve` keeps the local `gateway.yaml` default. The maintenance tasks default to `gateway.prod.yaml`; set `GATEWAY_CONFIG` if you want them to target a different config file.

`gateway serve` remains the default command. By default it reads `gateway.yaml` (or `GATEWAY_CONFIG`), runs SQL migrations, seeds providers/models/api keys, ensures a bootstrap admin exists, then starts serving traffic.

Database policy:

- `gateway.yaml` remains the local-development default and uses the libsql/SQLite backend.
- `gateway.prod.yaml` and deploy-facing configs use PostgreSQL by default.
- Fresh PostgreSQL environments are bootstrapped by the gateway on startup; application seed data is not preloaded into the Postgres container.

Bootstrap admin defaults:

- local config (`gateway.yaml`): `admin@local` / `admin`, no forced password change
- production helper config (`gateway.prod.yaml`): `admin@local` / `admin`, forced password change on first login

### Database config

Local libsql/SQLite example:

```yaml
database:
  kind: libsql
  path: "./gateway.db"
```

Production/pre-production PostgreSQL example:

```yaml
database:
  kind: postgres
  url: env.POSTGRES_URL
```

### Provider types

`providers[*].type` currently supports:

- `openai_compat`
- `gcp_vertex`

`openai_compat` requires a `pricing_provider_id` so the gateway can resolve exact pricing from the internal catalog. In this slice, supported values are:

- `openai`
- `google-vertex`
- `google-vertex-anthropic`

`gcp_vertex` supports three auth modes:

- `adc`
- `service_account` (`credentials_path`)
- `bearer` (`token`)

`gcp_vertex` routes require `upstream_model` in `<publisher>/<model_id>` format. In this slice, supported publishers are `google` and `anthropic`.

### Example Vertex config

```yaml
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
    auth:
      kind: bearer
      token: env.OPENAI_API_KEY
  - id: vertex-adc
    type: gcp_vertex
    project_id: your-gcp-project
    location: global
    auth:
      mode: adc
  - id: vertex-bearer
    type: gcp_vertex
    project_id: your-gcp-project
    location: global
    auth:
      mode: bearer
      token: env.GCP_VERTEX_BEARER_TOKEN

models:
  - id: fast
    description: Gemini on Vertex
    routes:
      - provider: vertex-adc
        upstream_model: google/gemini-2.0-flash
  - id: claude
    description: Claude on Vertex
    routes:
      - provider: vertex-bearer
        upstream_model: anthropic/claude-sonnet-4-6
```

## Setup

```bash
eval "$(/Users/ahstn/.local/bin/mise activate zsh)"
mise install
mise run ui-install
```

## Development

Run both UI and gateway together:

```bash
./scripts/start-dev-stack.sh
```

- Gateway/API: `http://localhost:8080`
- Admin UI: `http://localhost:8080/admin`
- Database backend: local libsql/SQLite via `gateway.yaml`

Run the gateway directly with the default startup behavior:

```bash
mise run gateway-serve
# or:
cargo run -p gateway --bin gateway -- serve
```

Inspect or apply migrations without starting the server:

```bash
cargo run -p gateway --bin gateway -- --config gateway.yaml migrate --status
cargo run -p gateway --bin gateway -- --config gateway.yaml migrate --check
GATEWAY_CONFIG=./gateway.yaml mise run gateway-migrate
```

## Production-style local run

```bash
docker compose -f compose.local.yaml up -d postgres
export POSTGRES_URL="postgres://oceans:oceans@localhost:5432/oceans_llm"
export OPENAI_API_KEY="${OPENAI_API_KEY:-test-openai-key}"
mise run ui-build
./scripts/start-prod.sh
```

`start-prod.sh` defaults `GATEWAY_CONFIG` to `./gateway.prod.yaml`, which now expects PostgreSQL through `POSTGRES_URL`, keeps the bootstrap admin enabled for first-time setup, and forces a password change after initial sign-in.

For one-off operational actions against the configured database:

```bash
mise run gateway-migrate
mise run gateway-seed-config
mise run gateway-bootstrap-admin
cargo run -p gateway --bin gateway -- --config gateway.prod.yaml serve --run-migrations=false --bootstrap-admin=false --seed-config=false
```

These maintenance tasks default to `gateway.prod.yaml`. Override `GATEWAY_CONFIG` if you need to point them at another config file.

## Postgres validation

Bring up the local Postgres service:

```bash
docker compose -f compose.local.yaml up -d postgres
export TEST_POSTGRES_URL="postgres://oceans:oceans@localhost:5432/oceans_llm"
export POSTGRES_URL="$TEST_POSTGRES_URL"
export OPENAI_API_KEY="${OPENAI_API_KEY:-test-openai-key}"
```

Libsql-first local validation:

```bash
mise run check
mise run test
```

Focused Postgres-backed validation:

```bash
mise run check-rust-postgres
mise run test-rust-postgres
mise run test-gateway-postgres-smoke
```

## Quality gates

```bash
mise run check
mise run lint
mise run test
mise run check-rust-postgres
mise run test-rust-postgres
mise run test-gateway-postgres-smoke
mise run sync-pricing-catalog
```

CI runs `mise run check-rust-postgres`, `mise run test-rust-postgres`, and `mise run test-gateway-postgres-smoke` so the PostgreSQL path stays visible in the workflow and exercised before merge.
