# Oceans LLM Gateway

Rust-first gateway workspace with an embedded `admin-ui` crate that hosts a TanStack Start control-plane UI.

## Workspace layout

- `crates/gateway`: Rust API binary (`/healthz`, `/readyz`, `/v1/*`)
- `crates/gateway-core`: shared domain types, traits, OpenAI-compatible DTOs, typed errors
- `crates/gateway-store`: Turso/libsql store, migrations, seed upserts
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
- `GATEWAY_CONFIG`: gateway config file path (default `./gateway.yaml`)
- `ADMIN_UI_BASE_PATH`: UI mount path (default `/admin`)
- `ADMIN_UI_UPSTREAM`: SSR upstream URL (default `http://localhost:3001`)
- `ADMIN_UI_CONNECT_TIMEOUT_MS`: Proxy connect timeout (default `750`)
- `ADMIN_UI_REQUEST_TIMEOUT_MS`: Proxy request timeout (default `10000`)
- `ADMIN_UI_INTERNAL_PORT`: Internal Bun SSR port used by helper scripts (default `3001`)

## Gateway config

`gateway` reads `gateway.yaml` (or `GATEWAY_CONFIG`) at startup, runs SQL migrations, seeds providers/models/api keys, then starts serving traffic.

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

## Production-style local run

```bash
mise run ui-build
./scripts/start-prod.sh
```

## Quality gates

```bash
mise run check
mise run lint
mise run test
mise run sync-pricing-catalog
```
