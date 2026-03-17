# Configuration Reference

`Owns`: gateway config shape, defaults, validation rules, provider-specific config constraints, and env-backed secret references.
`Depends on`: [../README.md](../README.md)
`See also`: [model-routing-and-api-behavior.md](model-routing-and-api-behavior.md), [pricing-catalog-and-accounting.md](pricing-catalog-and-accounting.md), [deploy-and-operations.md](deploy-and-operations.md), [adr/2026-03-10-model-aliases-and-provider-route-config.md](adr/2026-03-10-model-aliases-and-provider-route-config.md)

This page is the canonical reference for gateway YAML config. Runtime policy and API behavior live in neighboring docs; this page only owns the config contract itself.

## Source of Truth

- Config parsing and validation: [../crates/gateway/src/config.rs](../crates/gateway/src/config.rs)
- Provider capability defaults: [../crates/gateway-core/src/domain.rs](../crates/gateway-core/src/domain.rs)
- Checked-in examples:
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

Checked-in config supports literal values and env references.

Common patterns:

- `literal.admin`
- `env.OPENAI_API_KEY`
- `env.POSTGRES_URL`

Operationally, this means the YAML file defines structure while secrets and deploy-specific values should usually come from the environment.

## Defaults That Matter

Important defaults from config parsing and domain deserialization:

- model `rank` defaults to `100`
- route `priority` defaults to `100`
- route `weight` defaults to `1.0`
- route `enabled` defaults to `true`
- route capability flags default to all enabled
- Vertex `location` defaults to `global`
- Vertex `api_host` defaults to `aiplatform.googleapis.com`
- bootstrap admin defaults to enabled with `admin@local`

Those defaults materially affect routing and operator expectations, so they should be treated as part of the runtime contract rather than implementation trivia.

## `server`

Important fields:

- `bind`
- `log_format`
- `otel_endpoint`
- `otel_metrics_endpoint`
- `otel_export_interval_secs`

For observability semantics and current collector assumptions, see [observability-and-request-logs.md](observability-and-request-logs.md).

## `database`

Checked-in examples use two runtime shapes:

- local development: libsql/SQLite with `path`
- production-shaped and deploy flows: PostgreSQL with `kind: postgres` and `url`

For topology, migrations, and local-vs-deploy differences, see [deploy-and-operations.md](deploy-and-operations.md).

## `auth`

Important fields:

- `seed_api_keys`
- `bootstrap_admin`

Important distinctions:

- `seed_api_keys` is used in deploy-style or test-style setups to pre-create gateway keys
- `bootstrap_admin` controls whether the gateway ensures a platform admin exists at startup
- `bootstrap_admin.require_password_change` changes first-login behavior and is part of the live auth contract

Identity semantics are owned by [identity-and-access.md](identity-and-access.md).

## Provider Config

Supported provider types in the checked-in configs:

- `openai_compat`
- `gcp_vertex`

### `openai_compat`

Important fields:

- `id`
- `base_url`
- `pricing_provider_id`
- `auth.kind`
- `auth.token`

Validation rules that matter operationally:

- `pricing_provider_id` cannot be empty
- `pricing_provider_id` must be one of the supported internal pricing providers

That pricing mapping is part of accounting integrity, not just config hygiene. See [pricing-catalog-and-accounting.md](pricing-catalog-and-accounting.md).

### `gcp_vertex`

Important fields:

- `id`
- `project_id`
- `location`
- `api_host`
- `auth.mode`

Current checked-in auth examples:

- `adc`
- `service_account`

Routing and accounting caveats:

- `upstream_model` must use the `<publisher>/<model_id>` form
- pricing identity is inferred from the publisher prefix
- Anthropic-on-Vertex is only priced for `location=global`

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

Alias resolution and request-time behavior are owned by [model-routing-and-api-behavior.md](model-routing-and-api-behavior.md).

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

Capability flags default permissively, but runtime execution still intersects route config with provider adapter truth. A route can constrain provider capability, not expand it.

## Validation And Failure Boundaries

Config load catches several classes of failure up front:

- invalid or empty provider fields
- unsupported pricing-provider mappings
- invalid model alias references
- invalid route/provider wiring

At runtime, config shape is already fixed. Later failures are usually about request resolution, missing providers, capability mismatch, or pricing coverage.

## What This Page Does Not Own

- request routing and `/v1/*` behavior: [model-routing-and-api-behavior.md](model-routing-and-api-behavior.md)
- identity/onboarding semantics: [identity-and-access.md](identity-and-access.md)
- spend enforcement and budget windows: [budgets-and-spending.md](budgets-and-spending.md)
- pricing coverage and unpriced reasons: [pricing-catalog-and-accounting.md](pricing-catalog-and-accounting.md)
- deploy topology and release/ops workflow: [deploy-and-operations.md](deploy-and-operations.md), [release-process.md](release-process.md)
