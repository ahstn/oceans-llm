# Oceans LLM Gateway

<p align="center">
<img height="400" alt="oceans_llm_logo_v2" src="https://github.com/user-attachments/assets/37d617f1-3eb9-4774-bd38-7b7dd495eab4" />
</p>

Rust-first LLM gateway workspace with an embedded TanStack Start admin control plane.

## Overview

- `crates/gateway`
  - Rust HTTP runtime for `/healthz`, `/readyz`, `/v1/*`, and `/api/v1/admin/*`
- `crates/gateway-core`
  - shared domain types, traits, OpenAI-compatible DTOs, and errors
- `crates/gateway-store`
  - libsql or SQLite and PostgreSQL stores, migrations, and seed behavior
- `crates/gateway-service`
  - auth, model resolution, routing, accounting, and request logging
- `crates/gateway-providers`
  - provider adapters and transport helpers
- `crates/admin-ui`
  - Rust reverse-proxy integration for `/admin*`
- `crates/admin-ui/web`
  - TanStack Start and React admin UI

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

- gateway API: `http://localhost:8080`
- admin UI: `http://localhost:8080/admin`
- active config: `./gateway.yaml`
- database backend: local libsql or SQLite

## Core Commands

- local gateway:
  - `mise run gateway-serve`
- explicit migration:
  - `mise run gateway-migrate`
- explicit bootstrap admin:
  - `mise run gateway-bootstrap-admin`
- explicit config seed:
  - `mise run gateway-seed-config`
- admin contract generation:
  - `mise run admin-contract-generate`
- admin contract drift check:
  - `mise run admin-contract-check`
- full lint:
  - `mise run lint`
- full test:
  - `mise run test`
- E2E contract suite:
  - `mise run e2e-test`

## Documentation Map

Use the docs site instead of treating this file as the full operator manual.

- repo workflow:
  - [Contributing](CONTRIBUTING.md)
- docs site:
  - [Documentation Home](docs/index.md)
- startup and first access:
  - [Runtime Bootstrap and Access](docs/setup/runtime-bootstrap-and-access.md)
- config contract:
  - [Configuration Reference](docs/configuration/configuration-reference.md)
- identity:
  - [Identity and Access](docs/access/identity-and-access.md)
- routing:
  - [Model Routing and API Behavior](docs/configuration/model-routing-and-api-behavior.md)
- cross-cutting request flow:
  - [Request Lifecycle and Failure Modes](docs/reference/request-lifecycle-and-failure-modes.md)
- pricing and spend:
  - [Pricing Catalog and Accounting](docs/configuration/pricing-catalog-and-accounting.md)
  - [Budgets and Spending](docs/operations/budgets-and-spending.md)
- observability:
  - [Observability and Request Logs](docs/operations/observability-and-request-logs.md)
- admin UI:
  - [Admin Control Plane](docs/access/admin-control-plane.md)
- maintainer-facing docs source notes:
  - [Documentation Source Notes](docs/README.md)
- deploy quick start:
  - [Deploy Compose](deploy/README.md)

## Same-Origin Runtime Model

The product runs as a same-origin control plane:

1. the gateway listens on the configured bind address
2. the admin UI SSR process runs separately
3. the gateway reverse-proxies `/admin*` to the admin UI upstream

This is part of the product contract, not only a local-dev trick.

## Admin Contract Generation

The live admin control plane ships checked-in contract artifacts:

- gateway OpenAPI artifact:
  - `crates/gateway/openapi/admin-api.json`
- generated admin UI types:
  - `crates/admin-ui/web/src/generated/admin-api.ts`

Regenerate them with:

```bash
mise run admin-contract-generate
```

Verify drift with:

```bash
mise run admin-contract-check
```

For the full maintainer workflow, use [Admin API Contract Workflow](docs/reference/admin-api-contract-workflow.md).
