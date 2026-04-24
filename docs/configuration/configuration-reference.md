# Configuration Reference

`See also`: [Oceans LLM Gateway](../../README.md), [Runtime Bootstrap and Access](../setup/runtime-bootstrap-and-access.md), [Model Routing and API Behavior](model-routing-and-api-behavior.md), [Pricing Catalog and Accounting](pricing-catalog-and-accounting.md), [OIDC and SSO Status](../access/oidc-and-sso-status.md)

This page owns config syntax and parse-time rules. It does not own the full runtime story after a request starts moving.

## Source of Truth

- config parsing and validation:
  - [../crates/gateway/src/config.rs](../../crates/gateway/src/config.rs)
- provider capability defaults:
  - [../crates/gateway-core/src/domain.rs](../../crates/gateway-core/src/domain.rs)
- checked-in examples:
  - [../gateway.yaml](../../gateway.yaml)
  - [../gateway.prod.yaml](../../gateway.prod.yaml)
  - [../deploy/config/gateway.yaml](../../deploy/config/gateway.yaml)

## Top-Level Sections

- `server`
- `database`
- `auth`
- `budget_alerts`
- `request_logging`
- `providers`
- `models`
- `teams`
- `users`

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
    password: "literal.admin"
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
    password: env.GATEWAY_BOOTSTRAP_ADMIN_PASSWORD
    require_password_change: true
  seed_api_keys:
    - name: "gateway"
      value: env.GATEWAY_API_KEY

providers:
  - id: vertex
    type: gcp_vertex
    project_id: env.GCP_PROJECT_ID
    location: global
    auth:
      mode: service_account
      credentials_path: env.GCP_SERVICE_ACCOUNT_JSON

teams:
  - key: platform
    name: Platform
    budget:
      cadence: monthly
      amount_usd: "500.0000"
      hard_limit: true
      timezone: UTC

users:
  - name: Platform Admin
    email: ops@example.com
    auth_mode: password
    global_role: platform_admin
    membership:
      team: platform
      role: admin
    budget:
      cadence: monthly
      amount_usd: "100.0000"
      hard_limit: true
      timezone: UTC

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
- `request_logging.payloads.capture_mode` defaults to `redacted_payloads`
- `request_logging.payloads.request_max_bytes` defaults to `65536`
- `request_logging.payloads.response_max_bytes` defaults to `65536`
- `request_logging.payloads.stream_max_events` defaults to `128`

The startup meaning of bootstrap-admin and seeded API keys lives in [runtime-bootstrap-and-access.md](../setup/runtime-bootstrap-and-access.md).

## `server`

Important fields:

- `bind`
- `log_format`
- `otel_endpoint`
- `otel_metrics_endpoint`
- `otel_export_interval_secs`

For collector assumptions and request-log implications, see [observability-and-request-logs.md](../operations/observability-and-request-logs.md).

## `request_logging`

`request_logging.payloads` controls chat-completion request-log payload persistence.

```yaml
request_logging:
  payloads:
    capture_mode: redacted_payloads
    request_max_bytes: 65536
    response_max_bytes: 65536
    stream_max_events: 128
    redaction_paths: []
```

Important fields:

- `capture_mode`
  - `disabled`: skip chat-completion request-log persistence
  - `summary_only`: write summary rows with `has_payload=false` and no payload row
  - `redacted_payloads`: write summary rows and sanitized payload rows
- `request_max_bytes`: final persisted request payload budget
- `response_max_bytes`: final persisted response payload budget
- `stream_max_events`: maximum stored stream events; stream usage and error parsing still sees later frames
- `redaction_paths`: additive operator redaction paths anchored from the wrapped payload root

Validation rules:

- byte limits must be greater than zero
- `stream_max_events` must be greater than zero
- `redaction_paths` use dot-separated object keys plus `*` as a full-segment wildcard
- malformed paths such as `body..messages` or indexed paths such as `body.messages[0]` are rejected at config parse time

The runtime redaction/truncation policy and admin display behavior are owned by [observability-and-request-logs.md](../operations/observability-and-request-logs.md).

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
- `bootstrap_admin.password` must be `literal.*` or `env.*`

For startup behavior and first access after boot, use [runtime-bootstrap-and-access.md](../setup/runtime-bootstrap-and-access.md).

## Declarative Teams And Users

`teams` and `users` extend the same startup seed path used for providers, models, and API keys.

Important `teams` fields:

- `key`
- `name`
- `budget`

Important `users` fields:

- `name`
- `email`
- `auth_mode`
- `global_role`
- `request_logging_enabled`
- `oidc_provider_key`
- `membership.team`
- `membership.role`
- `budget`

Validation rules that matter:

- team keys must be unique
- `system-legacy` is reserved and cannot be configured
- user emails are normalized and must be unique
- `admin@local` is reserved for the bootstrap admin
- `users[*].auth_mode` supports `password` and `oidc`
- `oidc_provider_key` is required for `oidc` users and rejected for `password` users
- membership roles can be `admin` or `member`
- membership role `owner` is rejected
- budget amounts must be non-negative

Seed semantics that matter:

- listed teams are upserted by `teams[*].key`
- listed users are upserted by normalized email
- new config-seeded users are created as `invited`
- listed membership and active-budget state is reconciled for listed users and teams
- omitting a `budget` block for a listed user or team deactivates that owner's active budget
- unlisted teams and users are left untouched

OIDC provider existence is validated at seed time against enabled runtime OIDC providers, not YAML parse time.

## Provider Types

Supported provider types in the checked-in configs:

- `openai_compat`
- `gcp_vertex`

### Provider Auth Modes

| Provider type | Auth field | Expected secret material |
| --- | --- | --- |
| `openai_compat` | `auth.token` | bearer-style token |
| `gcp_vertex` | `auth.mode: adc` | ADC available in the runtime environment |
| `gcp_vertex` | `auth.mode: service_account` | `credentials_path` pointing at service-account JSON or an equivalent mounted secret path |

### `openai_compat`

Important fields:

- `id`
- `base_url`
- `pricing_provider_id`
- `auth.kind`
- `auth.token`
- optional `display.label`
- optional `display.icon_key`

`display.icon_key` currently accepts the checked-in provider icon codes used by the admin UI:

- `openai`
- `openrouter`
- `anthropic`
- `aws`
- `vertexai`

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
- optional `display.label`
- optional `display.icon_key`

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
- `compatibility`
- `extra_headers`
- `extra_body`

Capability flags default permissively. A route can constrain provider capability. It cannot expand provider truth.

Compatibility metadata is separate from capabilities. Capabilities decide whether a route may execute; compatibility describes explicit request and stream-shape transforms for the selected provider route.

Capability flags include API-family gates such as `chat_completions`, `responses`, and `embeddings`, plus feature gates such as `stream`, `tools`, `vision`, `json_schema`, and `developer_role`.

OpenAI-compatible route profile:

```yaml
models:
  - id: fast
    routes:
      - provider: openrouter
        upstream_model: openai/gpt-4o-mini
        compatibility:
          openai_compat:
            supports_store: false
            max_tokens_field: max_tokens
            developer_role: system
            reasoning_effort: omit
            supports_stream_usage: true
```

OpenAI-compatible profile defaults:

| Field | Default | Supported values |
| --- | --- | --- |
| `supports_store` | `true` | `true`, `false` |
| `max_tokens_field` | `max_completion_tokens` | `max_completion_tokens`, `max_tokens` |
| `developer_role` | `developer` | `developer`, `system` |
| `reasoning_effort` | `passthrough` | `passthrough`, `omit`, `reasoning_object` |
| `supports_stream_usage` | `false` | `true`, `false` |

The current `openai_compat` profile fields are Chat Completions transforms. `/v1/responses` is a separate supported API family and is not adapted by reusing Chat Completions compatibility shims.

Do not use `extra_body` for compatibility transforms. `extra_body` remains for additive provider-specific overrides, and the typed compatibility profile remains authoritative when a declared transform conflicts with an additive override.

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
  - [runtime-bootstrap-and-access.md](../setup/runtime-bootstrap-and-access.md)
- request routing and `/v1/*` behavior:
  - [model-routing-and-api-behavior.md](model-routing-and-api-behavior.md)
- cross-cutting request cause and effect:
  - [request-lifecycle-and-failure-modes.md](../reference/request-lifecycle-and-failure-modes.md)
- spend windows and budget policy:
  - [budgets-and-spending.md](../operations/budgets-and-spending.md)
- hardened OIDC status:
  - [oidc-and-sso-status.md](../access/oidc-and-sso-status.md)
