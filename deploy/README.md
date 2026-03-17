# Deploy Compose

`Owns`: the GHCR-based compose deployment entrypoint in `deploy/`.
`Depends on`: [../README.md](../README.md), [../docs/deploy-and-operations.md](../docs/deploy-and-operations.md)
`See also`: [../docs/model-routing-and-api-behavior.md](../docs/model-routing-and-api-behavior.md), [../docs/identity-and-access.md](../docs/identity-and-access.md)

This directory contains the user-facing Docker Compose setup that pulls the published images from GHCR:

- `ghcr.io/ahstn/oceans-llm-gateway`
- `ghcr.io/ahstn/oceans-llm-admin-ui`

## Files

- `compose.yaml`: gateway, admin UI, and PostgreSQL
- `config/gateway.yaml`: mounted production-shaped gateway config
- `.env.example`: image-tag and secret inputs

## Usage

```bash
cp deploy/.env.example deploy/.env
docker compose -f deploy/compose.yaml up -d
```

Default local endpoint:

- gateway: `http://localhost:8080`
- admin UI: `http://localhost:8080/admin`

## What This Stack Assumes

- PostgreSQL is the runtime database
- the gateway applies migrations and idempotent startup seed behavior on boot
- the Postgres container is not preloaded with application rows through `docker-entrypoint-initdb.d`
- this stack is compose-oriented and does not document every runtime/auth variation by itself

For the wider runtime and policy model, use the canonical docs instead of this deploy note:

- [Identity and Access](../docs/identity-and-access.md)
- [Configuration Reference](../docs/configuration-reference.md)
- [Model Routing and API Behavior](../docs/model-routing-and-api-behavior.md)
- [Pricing Catalog and Accounting](../docs/pricing-catalog-and-accounting.md)
- [Budgets and Spending](../docs/budgets-and-spending.md)
- [Observability and Request Logs](../docs/observability-and-request-logs.md)
- [Deploy and Operations](../docs/deploy-and-operations.md)
