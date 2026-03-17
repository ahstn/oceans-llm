# Deploy and Operations

`Owns`: runtime topology, local-vs-deploy differences, bootstrap/seed expectations, and deployment-time operational caveats.
`Depends on`: [../README.md](../README.md), [configuration-reference.md](configuration-reference.md)
`See also`: [../deploy/README.md](../deploy/README.md), [identity-and-access.md](identity-and-access.md), [observability-and-request-logs.md](observability-and-request-logs.md), [release-process.md](release-process.md)

This page owns the runtime topology and operator caveats that are otherwise easy to miss when reading only a compose file or startup script.

## Runtime Topology

The product runs as a same-origin control plane:

1. the gateway serves `/v1/*`, admin APIs, and `/admin*`
2. the admin UI SSR server runs separately
3. the gateway reverse-proxies `/admin*` to `ADMIN_UI_UPSTREAM`

This same-origin model is part of the product contract, not just a local-dev trick.

## Common Runtime Shapes

### Local development

- config: [../gateway.yaml](../gateway.yaml)
- database: libsql/SQLite
- admin bootstrap: enabled, no forced password change
- typical entry point: [../scripts/start-dev-stack.sh](../scripts/start-dev-stack.sh)

### Production-shaped local run

- config: [../gateway.prod.yaml](../gateway.prod.yaml)
- database: PostgreSQL
- admin bootstrap: enabled with forced password rotation
- entry point: [../scripts/start-prod.sh](../scripts/start-prod.sh)

### GHCR compose deployment

- compose: [../deploy/compose.yaml](../deploy/compose.yaml)
- config: [../deploy/config/gateway.yaml](../deploy/config/gateway.yaml)
- database: PostgreSQL
- gateway auth bootstrap: seeded API key, no checked-in bootstrap admin block

That means the compose deployment path and the production-shaped local path do not have the same initial auth story.

## Bootstrap vs Seeded Access

There are two distinct startup patterns in this repo:

- bootstrap admin creation for the control plane
- seeded API key creation for API access

Production-shaped local runs emphasize bootstrap admin login.

The GHCR compose deployment emphasizes seeded API access via `GATEWAY_API_KEY` and leaves admin bootstrap behavior to the mounted config and runtime settings you provide.

For the identity contract behind these choices, see [identity-and-access.md](identity-and-access.md).

## Admin UI Local Development

Direct UI development on `:3001` still depends on the gateway:

- server-side admin loaders call back into gateway APIs
- same-origin behavior is the normal contract
- changing gateway routes or backend response contracts can require a gateway restart even when the UI is running separately

This is why admin UI changes can still break cross-layer behavior without touching visible page code.

## Observability Wiring

Observability is OTLP-first.

Relevant config knobs:

- `server.otel_endpoint`
- `server.otel_metrics_endpoint`
- `server.otel_export_interval_secs`

Current repo deploy files do not ship a collector by default. Operators should decide whether they are:

- running with OTLP export configured to an external collector, or
- running without a collector and relying on logs plus request-log storage

For request-log semantics and known limits, see [observability-and-request-logs.md](observability-and-request-logs.md).

## Database And Migration Notes

PostgreSQL is the intended production and pre-production runtime backend.

Operationally important context:

- the gateway can run migrations on startup
- startup can also seed config and bootstrap auth state
- pre-v1 migration flattening is still tracked as follow-up work in [issue #44](https://github.com/ahstn/oceans-llm/issues/44)

## What This Page Does Not Own

- compose file usage details: [../deploy/README.md](../deploy/README.md)
- release authoring and tag workflow: [release-process.md](release-process.md)
- request routing and API semantics: [model-routing-and-api-behavior.md](model-routing-and-api-behavior.md)
