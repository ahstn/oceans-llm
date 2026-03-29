# Configuration Reference

`Owns`: gateway YAML shape, defaults, validation rules, provider auth modes, and env-backed secret references.
`Depends on`: [../README.md](../README.md)
`See also`: [runtime-bootstrap-and-access.md](runtime-bootstrap-and-access.md), [model-routing-and-api-behavior.md](model-routing-and-api-behavior.md), [pricing-catalog-and-accounting.md](pricing-catalog-and-accounting.md), [oidc-and-sso-status.md](oidc-and-sso-status.md)

This page owns config syntax and parse-time rules. It does not own the full runtime story after a request starts moving.

## Source of Truth

- config parsing and validation:
  - [../crates/gateway/src/config.rs](../crates/gateway/src/config.rs)
- provider capability defaults:
  - [../crates/gateway-core/src/domain.rs](../crates/gateway-core/src/domain.rs)
- checked-in examples:
  - [../gateway.yaml](../gateway.yaml)
  - [../gateway.prod.yaml](../gateway.prod.yaml)
  - [../deploy/config/gateway.yaml](../deploy/config/gateway.yaml)

## Top-Level Sections

- `server`
- `database`
- `auth`
- `providers`
- `models`

## Value Sources

The config supports literal values and env references.

Common patterns:

- `literal.admin`
- `env.OPENAI_API_KEY`
- `env.POSTGRES_URL`

The YAML holds structure. Secrets and deploy-specific values usually come from the environment.

## Minimal Local Example

```yaml
server:
  bind: "127.0.0.1:8080"

database:
  kind: libsql
  path: "./var/oceans.db"

auth:
  bootstrap_admin:
    email: "admin@local"
    password: "admin"
    require_password_change: false

providers:
  - id: openai
    type: openai_compat
    base_url: "https://api.openai.com/v1"
    pricing_provider_id: openai
    auth:
      kind: bearer
      token: env.OPENAI_API_KEY

models:
  - id: gpt-4o-mini
    routes:
      - provider: openai
        upstream_model: gpt-4o-mini
```

## Production-Shaped Example

```yaml
server:
  bind: "0.0.0.0:8080"

database:
  kind: postgres
  url: env.POSTGRES_URL

auth:
  bootstrap_admin:
    email: "admin@local"
    password: "admin"
    require_password_change: true
  seed_api_keys:
    - name: "gateway"
      key: env.GATEWAY_API_KEY

providers:
  - id: vertex
    type: gcp_vertex
    project_id: env.GCP_PROJECT_ID
    location: global
    auth:
      mode: service_account
      service_account_json: env.GCP_SERVICE_ACCOUNT_JSON

models:
  - id: gemini-2.0-flash
    routes:
      - provider: vertex
        upstream_model: google/gemini-2.0-flash
```

The checked-in examples are opinionated. They are not the full config space.

## Defaults That Matter

Important defaults from config parsing and domain deserialization:

- model `rank` defaults to `100`
- route `priority` defaults to `100`
- route `weight` defaults to `1.0`
- route `enabled` defaults to `true`
- route capability flags default to all enabled
- Vertex `location` defaults to `global`
- Vertex `api_host` defaults to `aiplatform.googleapis.com`

The startup meaning of bootstrap-admin and seeded API keys lives in [runtime-bootstrap-and-access.md](runtime-bootstrap-and-access.md).

## `server`

Important fields:

- `bind`
- `log_format`
- `otel_endpoint`
- `otel_metrics_endpoint`
- `otel_export_interval_secs`

For collector assumptions and request-log implications, see [observability-and-request-logs.md](observability-and-request-logs.md).

## `database`

The checked-in configs use two runtime shapes:

- local development:
  - libsql or SQLite with `path`
- production-shaped and deploy flows:
  - PostgreSQL with `kind: postgres` and `url`

## `auth`

Important fields:

- `seed_api_keys`
- `bootstrap_admin`

Important distinctions:

- `seed_api_keys` creates data-plane access
- `bootstrap_admin` creates control-plane access
- `bootstrap_admin.require_password_change` changes first-login behavior

For startup behavior and first access after boot, use [runtime-bootstrap-and-access.md](runtime-bootstrap-and-access.md).

## Provider Types

Supported provider types in the checked-in configs:

- `openai_compat`
- `gcp_vertex`

### Provider Auth Modes

| Provider type | Auth field | Expected secret material |
| --- | --- | --- |
| `openai_compat` | `auth.token` | bearer-style token |
| `gcp_vertex` | `auth.mode: adc` | ADC available in the runtime environment |
| `gcp_vertex` | `auth.mode: service_account` | service-account JSON or equivalent secret source |

### `openai_compat`

Important fields:

- `id`
- `base_url`
- `pricing_provider_id`
- `auth.kind`
- `auth.token`

Validation rules that matter:

- `pricing_provider_id` cannot be empty
- `pricing_provider_id` must map to a supported internal pricing family

### `gcp_vertex`

Important fields:

- `id`
- `project_id`
- `location`
- `api_host`
- `auth.mode`

Routing and pricing caveats:

- `upstream_model` must use `<publisher>/<model_id>`
- pricing identity is inferred from the publisher prefix
- Anthropic-on-Vertex pricing is only supported for `location=global`

## Model Config

Configured gateway models are either:

- provider-backed models with `routes`
- alias-backed models with `alias_of`

A model cannot be both.

Important fields:

- `id`
- `description`
- `tags`
- `rank`
- `routes`
- `alias_of`

## Route Config

Important fields:

- `provider`
- `upstream_model`
- `priority`
- `weight`
- `enabled`
- `capabilities`
- `extra_headers`
- `extra_body`

Capability flags default permissively. A route can constrain provider capability. It cannot expand provider truth.

## Validation and Failure Boundaries

Config load catches several classes of failure up front:

- invalid or empty provider fields
- unsupported pricing-provider mappings
- invalid alias references
- invalid route or provider wiring

Later failures are usually runtime problems such as:

- request resolution failure
- missing providers
- capability mismatch
- exact-pricing gaps

## Current Gaps

- Declarative teams, users, and budgets are not part of the config contract yet.
- The future direction is tracked in [issue #64](https://github.com/ahstn/oceans-llm/issues/64) and [issue #65](https://github.com/ahstn/oceans-llm/issues/65).

## What This Page Does Not Own

- startup behavior and first access:
  - [runtime-bootstrap-and-access.md](runtime-bootstrap-and-access.md)
- request routing and `/v1/*` behavior:
  - [model-routing-and-api-behavior.md](model-routing-and-api-behavior.md)
- cross-cutting request cause and effect:
  - [request-lifecycle-and-failure-modes.md](request-lifecycle-and-failure-modes.md)
- spend windows and budget policy:
  - [budgets-and-spending.md](budgets-and-spending.md)
- hardened OIDC status:
  - [oidc-and-sso-status.md](oidc-and-sso-status.md)
