# Oceans LLM Gateway

Rust-first LLM gateway workspace with an embedded TanStack Start admin control plane.

## Overview

- `crates/gateway`: Rust HTTP runtime for `/healthz`, `/readyz`, `/v1/*`, and `/api/v1/admin/*`
- `crates/gateway-core`: shared domain types, traits, OpenAI-compatible DTOs, and errors
- `crates/gateway-store`: libsql/SQLite and PostgreSQL stores, migrations, and seed behavior
- `crates/gateway-service`: auth, model resolution, routing, accounting, and request logging
- `crates/gateway-providers`: provider adapters and transport helpers
- `crates/admin-ui`: Rust reverse proxy integration for `/admin*`
- `crates/admin-ui/web`: TanStack Start + React admin UI

## Runtime Model

The repo runs as a same-origin control plane:

1. The gateway listens on `server.bind` from the active config file.
2. The admin UI SSR process runs separately, typically on internal port `3001`.
3. The gateway reverse-proxies `/admin*` to `ADMIN_UI_UPSTREAM`.

Checked-in configs keep the local-development default on libsql/SQLite and the production-shaped default on PostgreSQL.

## Docs Map

Use the canonical docs for behavior and policy details:

- [Documentation Hub](docs/README.md)
- [Identity and Access](docs/identity-and-access.md)
- [Model Routing and API Behavior](docs/model-routing-and-api-behavior.md)
- [Budgets and Spending](docs/budgets-and-spending.md)
- [Observability and Request Logs](docs/observability-and-request-logs.md)
- [Data Relationships](docs/data-relationships.md)
- [Admin Control Plane](docs/admin-control-plane.md)
- [End-to-End Contract Tests](docs/e2e-contract-tests.md)
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

For config semantics, model routing, aliases, capability gating, pricing-provider requirements, and request behavior, see [Model Routing and API Behavior](docs/model-routing-and-api-behavior.md).

## Production-Shaped Local Run

Start PostgreSQL and run the production-shaped config locally:

```bash
docker compose -f compose.local.yaml up -d postgres
export POSTGRES_URL="postgres://oceans:oceans@localhost:5432/oceans_llm"
export OPENAI_API_KEY="${OPENAI_API_KEY:-test-openai-key}"
mise run ui-build
./scripts/start-prod.sh
```

`start-prod.sh` defaults `GATEWAY_CONFIG` to `./gateway.prod.yaml`, uses PostgreSQL, keeps bootstrap-admin creation enabled, and forces password rotation for the default admin on first login.

For deploy-oriented usage, image tags, and compose layout, see [deploy/README.md](deploy/README.md).

## Validation

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
