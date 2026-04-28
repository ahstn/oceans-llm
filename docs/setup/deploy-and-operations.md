# Deploy and Operations

`See also`: [Oceans LLM Gateway](../../README.md), [Configuration Reference](../configuration/configuration-reference.md), [Runtime Bootstrap and Access](runtime-bootstrap-and-access.md), [Admin Runbooks](../operations/operator-runbooks.md), [Deploy](../../deploy/README.md), [Release Process](../reference/release-process.md)

This page owns the runtime shape. It does not own the action-by-action runbooks.

## Runtime Topology

The product runs as a same-origin control plane:

1. the gateway serves `/v1/*`, admin APIs, and `/admin*`
2. the admin UI SSR server runs separately
3. the gateway reverse-proxies `/admin*` to `ADMIN_UI_UPSTREAM`

This is a product constraint, not only a local-dev convenience.

## Common Runtime Shapes

### Local development

- config:
  - [../gateway.yaml](../../gateway.yaml)
- database:
  - libsql or SQLite
- entry point:
  - `mise run dev-stack`
- bootstrap admin:
  - enabled, no forced password change

### Production-shaped local run

- config:
  - [../gateway.prod.yaml](../../gateway.prod.yaml)
- database:
  - PostgreSQL
- entry point:
  - `mise run prod-stack`
- bootstrap admin:
  - enabled, forced password rotation

### GHCR compose deployment

- compose:
  - [../deploy/compose.yaml](../../deploy/compose.yaml)
- config:
  - [../deploy/config/gateway.yaml](../../deploy/config/gateway.yaml)
- database:
  - PostgreSQL
- checked-in first access:
  - seeded API key and env-backed bootstrap admin password

That means the compose deploy path and the production-shaped local path both create a bootstrap admin, but the credential source differs. The production-shaped local path uses the local config defaults; compose expects deploy-time environment secrets.

### Kubernetes Helm deployment

- chart source:
  - [../../deploy/helm/oceans-llm](../../deploy/helm/oceans-llm/README.md)
- published chart:
  - `oci://ghcr.io/ahstn/charts/oceans-llm`
- config:
  - `gateway.config` values rendered to `gateway.configMountPath` (default: `/app/gateway.yaml`)
- database:
  - external PostgreSQL by default, or an optional CloudNativePG `Cluster` when those CRDs already exist
- checked-in first access:
  - migration Job enabled by default
  - bootstrap-admin and seed-config Jobs disabled until explicitly enabled

The Helm path keeps all public traffic pointed at the gateway service. It does not expose the admin UI service through ingress.

## Why The Same-Origin Model Matters

The same-origin model changes more than routing.

It affects:

- deploy topology
- admin UI local debugging
- E2E scope
- release risk when backend contract changes land

## Admin UI Local Development

Direct UI work on `:3001` still depends on the gateway:

- server-side admin loaders call back into gateway APIs
- same-origin behavior is still the normal contract
- backend route changes can require a gateway restart even when the UI server is already running

## Observability Wiring

Observability is OTLP-first.

Relevant config knobs:

- `server.otel_endpoint`
- `server.otel_metrics_endpoint`
- `server.otel_export_interval_secs`

The checked-in deploy path does not ship a collector by default.

The Helm chart follows the same rule: it exposes env, annotations, labels, volumes, and sidecar hooks, but does not bundle a collector or vendor agent.

## Database and Migration Notes

PostgreSQL is the intended production and pre-production runtime backend.

Operationally important context:

- the gateway can run migrations on startup
- startup can also seed config and bootstrap auth state
- fresh databases apply a single `V17` baseline per backend
- `status`, `check`, and `apply` validate `refinery_schema_history` against the active registry before proceeding
- databases carrying pre-baseline `V1` through `V16` history must be recreated instead of upgraded in place

## Image and Release Caveats

Current image support is not symmetric:

- gateway image:
  - `linux/amd64`
- admin UI image:
  - `linux/amd64`
  - `linux/arm64`

Release mechanics live in [release-process.md](../reference/release-process.md). Upgrade and recovery steps live in [operator-runbooks.md](../operations/operator-runbooks.md).

The release workflow also publishes the Helm chart after both deployable images build successfully. Consumers install the chart from `oci://ghcr.io/ahstn/charts/oceans-llm` with an explicit chart version.

## What This Page Does Not Own

- startup behavior and first access:
  - [runtime-bootstrap-and-access.md](runtime-bootstrap-and-access.md)
- compose quick start:
  - [../deploy/README.md](../../deploy/README.md)
- Kubernetes chart contract:
  - [kubernetes-and-helm.md](kubernetes-and-helm.md)
- action-oriented recovery:
  - [operator-runbooks.md](../operations/operator-runbooks.md)
- request routing semantics:
  - [model-routing-and-api-behavior.md](../configuration/model-routing-and-api-behavior.md)
