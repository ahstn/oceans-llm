# Deploy and Operations

`See also`: [Oceans LLM Gateway](../../README.md), [Configuration Reference](../configuration/configuration-reference.md), [Runtime Bootstrap and Access](runtime-bootstrap-and-access.md), [Operator Runbooks](../operations/operator-runbooks.md), [Deploy Compose](../../deploy/README.md), [Release Process](../reference/release-process.md)

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
  - [../scripts/start-dev-stack.sh](../../scripts/start-dev-stack.sh)
- bootstrap admin:
  - enabled, no forced password change

### Production-shaped local run

- config:
  - [../gateway.prod.yaml](../../gateway.prod.yaml)
- database:
  - PostgreSQL
- entry point:
  - [../scripts/start-prod.sh](../../scripts/start-prod.sh)
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
  - seeded API key, no checked-in bootstrap-admin block

That means the compose deploy path and the production-shaped local path do not teach the same first-access story.

### Public docs site

- source tree:
  - [../index.md](../index.md)
  - [../.vitepress/config.mts](../.vitepress/config.mts)
- build artifact:
  - `docs/.vitepress/dist`
- deploy target:
  - Cloudflare Pages project `oceans-llm-docs`
- canonical hostname:
  - `https://oceans-llm.com`
- release trigger:
  - [../.github/workflows/release.yml](../../.github/workflows/release.yml)

The public docs site is a separate static delivery surface. It is not served by the gateway runtime or the admin UI runtime.

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

## Database and Migration Notes

PostgreSQL is the intended production and pre-production runtime backend.

Operationally important context:

- the gateway can run migrations on startup
- startup can also seed config and bootstrap auth state
- pre-v1 migration flattening is still tracked as follow-up work

## Image and Release Caveats

Current image support is not symmetric:

- gateway image:
  - `linux/amd64`
- admin UI image:
  - `linux/amd64`
  - `linux/arm64`

Docs release mechanics are separate from the application images:

- the VitePress site is built from `docs/`
- release tags deploy it to Cloudflare Pages
- Cloudflare custom domains and the `www` to apex redirect must already be configured outside the repo

Release mechanics live in [release-process.md](../reference/release-process.md). Upgrade and recovery steps live in [operator-runbooks.md](../operations/operator-runbooks.md).

## What This Page Does Not Own

- startup behavior and first access:
  - [runtime-bootstrap-and-access.md](runtime-bootstrap-and-access.md)
- compose quick start:
  - [../deploy/README.md](../../deploy/README.md)
- action-oriented recovery:
  - [operator-runbooks.md](../operations/operator-runbooks.md)
- request routing semantics:
  - [model-routing-and-api-behavior.md](../configuration/model-routing-and-api-behavior.md)
