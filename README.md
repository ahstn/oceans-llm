# Oceans LLM Gateway

<p align="center">
<img height="400" alt="oceans_llm_logo_v2" src="https://github.com/user-attachments/assets/37d617f1-3eb9-4774-bd38-7b7dd495eab4" />
</p>

Rust-first LLM gateway workspace with an embedded TanStack Start admin control plane.

## Overview


- `crates/gateway`: Rust HTTP runtime for `/healthz`, `/readyz`, `/v1/*`, and `/api/v1/admin/*`
- `crates/gateway-core`: shared domain types, traits, OpenAI-compatible DTOs, and errors
- `crates/gateway-store`: libsql/SQLite and PostgreSQL stores, migrations, and seed behavior
- `crates/gateway-service`: auth, model resolution, routing, accounting, and request logging
- `crates/gateway-providers`: provider adapters and transport helpers
- `crates/admin-ui`: Rust reverse proxy integration for `/admin*`
- `crates/admin-ui/web`: TanStack Start + React admin UI



## Gateway config

- `PORT`: helper-script/container env used when launching the gateway process (the gateway listener itself comes from `server.bind` in the active config)
- `GATEWAY_CONFIG`: gateway config file path (default `./gateway.yaml`, prod helper uses `./gateway.prod.yaml`)
- `GATEWAY_RUN_MIGRATIONS`: control `gateway serve --run-migrations` (default `true`)
- `GATEWAY_BOOTSTRAP_ADMIN`: control `gateway serve --bootstrap-admin` (default `true`)
- `GATEWAY_SEED_CONFIG`: control `gateway serve --seed-config` (default `true`)
- `POSTGRES_URL`: PostgreSQL connection string used by production-shaped configs (for example `postgres://oceans:oceans@localhost:5432/oceans_llm`)
- `TEST_POSTGRES_URL`: PostgreSQL connection string used by Postgres-focused test helpers (defaults to `POSTGRES_URL` in the local pitchfork flow)
- `OCEANS_POSTGRES_HOST` / `OCEANS_POSTGRES_PORT` / `OCEANS_POSTGRES_DB` / `OCEANS_POSTGRES_USER` / `OCEANS_POSTGRES_PASSWORD`: local pitchfork Postgres defaults used to derive `POSTGRES_URL` and `TEST_POSTGRES_URL`
- `ADMIN_UI_BASE_PATH`: UI mount path (default `/admin`)
- `ADMIN_UI_UPSTREAM`: SSR upstream URL (default `http://localhost:3001`)
- `ADMIN_UI_CONNECT_TIMEOUT_MS`: Proxy connect timeout (default `750`)
- `ADMIN_UI_REQUEST_TIMEOUT_MS`: Proxy request timeout (default `10000`)
- `ADMIN_UI_INTERNAL_PORT`: Internal Bun SSR port used by helper scripts (default `3001`)


## Runtime Model

The repo runs as a same-origin control plane:

1. The gateway listens on `server.bind` from the active config file.
2. The admin UI SSR process runs separately, typically on internal port `3001`.
3. The gateway reverse-proxies `/admin*` to `ADMIN_UI_UPSTREAM`.

Checked-in configs keep the local-development default on libsql/SQLite and the production-shaped default on PostgreSQL.

## Docs Map

Use the canonical docs for behavior and policy details:

- [Contributing](CONTRIBUTING.md)
- [Documentation Hub](docs/README.md)
- [Configuration Reference](docs/configuration-reference.md)
- [Identity and Access](docs/identity-and-access.md)
- [Model Routing and API Behavior](docs/model-routing-and-api-behavior.md)
- [Budgets and Spending](docs/budgets-and-spending.md)
- [Pricing Catalog and Accounting](docs/pricing-catalog-and-accounting.md)
- [Observability and Request Logs](docs/observability-and-request-logs.md)
- [Data Relationships](docs/data-relationships.md)
- [Admin Control Plane](docs/admin-control-plane.md)
- [End-to-End Contract Tests](docs/e2e-contract-tests.md)
- [Deploy and Operations](docs/deploy-and-operations.md)
- [Release Process](docs/release-process.md)
- [Deploy Compose](deploy/README.md)

## Quick Start

Install the repo toolchain:

```bash
eval "$(mise activate zsh)"
mise install
mise run ui-install
```

Run the local development stack:

```bash
./scripts/start-dev-stack.sh
```

Default local endpoints:

- Gateway API: `http://localhost:8080`
- Admin UI: `http://localhost:8080/admin`
- Active config: `./gateway.yaml`
- Database backend: local libsql/SQLite

For contributor setup, workspace layout, task conventions, CI workflow references, and editor recommendations, see [CONTRIBUTING.md](CONTRIBUTING.md).

## Admin Contract Generation

The live admin control plane now ships checked-in contract artifacts:

- gateway OpenAPI artifact: `crates/gateway/openapi/admin-api.json`
- generated admin UI types: `crates/admin-ui/web/src/generated/admin-api.ts`

Refresh them locally with:

```bash
mise run admin-contract-generate
```

Verify they are current with:

```bash
mise run admin-contract-check
```

`mise run lint` and CI both enforce this drift check.

## Gateway Commands

The runtime exposes explicit operational commands:

- `gateway serve`
- `gateway migrate --status|--check|--apply`
- `gateway bootstrap-admin`
- `gateway seed-config`

Matching `mise` tasks:

- `mise run gateway-serve`
- `mise run gateway-migrate`
- `mise run gateway-bootstrap-admin`
- `mise run gateway-seed-config`

`gateway serve` remains the default startup path. It reads `GATEWAY_CONFIG` or `./gateway.yaml`, runs migrations, seeds providers and models, ensures the bootstrap admin exists, and then starts serving traffic.

## Configuration Entry Points

Important env vars:

- `GATEWAY_CONFIG`: config file path, default `./gateway.yaml`
- `PORT`: helper-script/container port input
- `POSTGRES_URL`: PostgreSQL connection string for production-shaped configs
- `GATEWAY_RUN_MIGRATIONS`
- `GATEWAY_BOOTSTRAP_ADMIN`
- `GATEWAY_SEED_CONFIG`
- `ADMIN_UI_UPSTREAM`
- `ADMIN_UI_INTERNAL_PORT`

For config shape, defaults, provider-specific constraints, and env-backed secret references, see [Configuration Reference](docs/configuration-reference.md). For request behavior and routing semantics, see [Model Routing and API Behavior](docs/model-routing-and-api-behavior.md).

## Production-Shaped Local Run

Start PostgreSQL and run the production-shaped config locally:

Pitchfork-first local Postgres:

```bash
mise run postgres-start
eval "$(mise run postgres-env)"
export OPENAI_API_KEY="${OPENAI_API_KEY:-test-openai-key}"
mise run ui-build
./scripts/start-prod.sh
```

Default local values emitted by `mise run postgres-env`:

- `POSTGRES_URL=postgres://oceans:oceans@127.0.0.1:5432/oceans_llm`
- `TEST_POSTGRES_URL=postgres://oceans:oceans@127.0.0.1:5432/oceans_llm`

`start-prod.sh` defaults `GATEWAY_CONFIG` to `./gateway.prod.yaml`, which now expects PostgreSQL through `POSTGRES_URL`, keeps the bootstrap admin enabled for first-time setup, and forces a password change after initial sign-in.

For one-off operational actions against the configured database:

```bash
mise run gateway-migrate
mise run gateway-seed-config
mise run gateway-bootstrap-admin
cargo run -p gateway --bin gateway -- --config gateway.prod.yaml serve --run-migrations=false --bootstrap-admin=false --seed-config=false
```

These maintenance tasks default to `gateway.prod.yaml`. Override `GATEWAY_CONFIG` if you need to point them at another config file.

For deploy-oriented usage, image tags, and compose layout, see [deploy/README.md](deploy/README.md).

Bring up local Postgres (pitchfork-first):

```bash
mise run postgres-start
eval "$(mise run postgres-env)"
export OPENAI_API_KEY="${OPENAI_API_KEY:-test-openai-key}"
```

Libsql-first local validation:

```bash
mise run check
mise run lint
mise run test
```

Focused PostgreSQL validation:

```bash
docker compose -f compose.local.yaml up -d postgres
export TEST_POSTGRES_URL="postgres://oceans:oceans@localhost:5432/oceans_llm"
export POSTGRES_URL="$TEST_POSTGRES_URL"
export OPENAI_API_KEY="${OPENAI_API_KEY:-test-openai-key}"
mise run check-rust-postgres
mise run test-rust-postgres
mise run test-gateway-postgres-smoke
```

CI runs `mise run check-rust-postgres`, `mise run test-rust-postgres`, and `mise run test-gateway-postgres-smoke` so the PostgreSQL path stays visible in the workflow and exercised before merge.
`mise run sync-pricing-catalog` refreshes the vendored pricing snapshot used to seed model pricing history for deterministic spend accounting.

Release-readiness checklist for Postgres runtime parity:

- `mise run check-rust-postgres`
- `mise run test-rust-postgres`
- `mise run test-gateway-postgres-smoke`
