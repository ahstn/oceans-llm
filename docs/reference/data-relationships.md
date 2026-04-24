# Data Relationships

`See also`: [Identity and Access](../access/identity-and-access.md), [Model Routing and API Behavior](../configuration/model-routing-and-api-behavior.md), [Provider API Compatibility](provider-api-compatibility.md), [Budgets and Spending](../operations/budgets-and-spending.md), [Observability and Request Logs](../operations/observability-and-request-logs.md), [ADR: Identity Foundation for Users, Teams, and API Key Ownership](../adr/2026-03-05-identity-foundation.md), [ADR: Route-Level Provider API Compatibility Profiles](../adr/2026-04-23-route-level-provider-api-compatibility-profiles.md)

This document is schema-oriented. It describes the persistent relationships that are hard to infer from a single file, but it does not try to restate every runtime rule owned by neighboring docs.

## Source of Truth

- Migrations:
  - [../crates/gateway-store/migrations/](../../crates/gateway-store/migrations)
  - [../crates/gateway-store/migrations/postgres/](../../crates/gateway-store/migrations/postgres)
- Core types:
  - [../crates/gateway-core/src/domain.rs](../../crates/gateway-core/src/domain.rs)
  - [../crates/gateway-core/src/traits.rs](../../crates/gateway-core/src/traits.rs)
- Runtime behavior:
  - [../crates/gateway-service/src/model_access.rs](../../crates/gateway-service/src/model_access.rs)
  - [../crates/gateway-service/src/model_resolution.rs](../../crates/gateway-service/src/model_resolution.rs)
  - [../crates/gateway-service/src/request_logging.rs](../../crates/gateway-service/src/request_logging.rs)
  - [../crates/gateway-service/src/budget_guard.rs](../../crates/gateway-service/src/budget_guard.rs)

## Core Entity Graph

1. `teams` 1..N `team_memberships`
2. `users` 0..1 `team_memberships`
3. `api_keys` belongs to exactly one owner
4. `api_keys` N..N `gateway_models` through `api_key_model_grants`
5. Optional restriction overlays:
   - `user_model_allowlist`
   - `team_model_allowlist`
6. `user_budgets` and `team_budgets` each allow one active budget per owner
7. `usage_cost_events` records request ownership, model attribution, pricing status, and computed cost
8. `request_logs` records the final user-visible request outcome
9. `request_log_payloads` stores sanitized request and response bodies separately from the summary row
10. `pricing_catalog_cache` stores normalized pricing snapshots used by runtime pricing resolution
11. `model_pricing` stores effective-dated pricing rows used for historical charging
12. `usage_cost_event_duplicates_archive` preserves duplicate-ledger migration/archive context

## Table Catalog

### Foundation Tables

- `providers`: upstream provider config and secret references
- `gateway_models`: gateway model registry; rows can be provider-backed or alias-backed
- `model_routes`: execution targets for provider-backed models only
- `api_key_model_grants`: model grants attached to an API key
- `audit_logs`: control-plane audit baseline

`model_routes` stores two distinct route execution metadata documents:

- `capabilities_json` controls whether the route may execute a request
- `compatibility_json` controls declared provider API compatibility transforms after route selection

`capabilities_json` includes API-family gates such as `chat_completions`, `responses`, and `embeddings`.

Compatibility metadata is not a provider config fallback and is not an `extra_body` convention.

### Identity and Access Tables

- `teams`
  - Key columns: `team_id`, `team_key`, `status`, `model_access_mode`
  - Notes: `team_key` is the durable identifier; `model_access_mode` is `all|restricted`
- `users`
  - Key columns: `user_id`, `email`, `global_role`, `auth_mode`, `request_logging_enabled`, `model_access_mode`
  - Notes: case-insensitive uniqueness is enforced through `email_normalized`
- `team_memberships`
  - Key columns: `team_id`, `user_id`, `role`
  - Notes: one-team-per-user is enforced by a unique `user_id`
- `oidc_providers`
  - Key columns: `oidc_provider_id`, `provider_key`, `provider_type`, `issuer_url`, `client_id`, `enabled`
  - Notes: the current schema supports `okta|generic_oidc`
- `user_password_auth`
  - Key columns: `user_id`, `password_hash`, `password_updated_at`
- `user_oidc_auth`
  - Key columns: `user_id`, `oidc_provider_id`, `subject`, `email_claim`
  - Notes: unique `(oidc_provider_id, subject)`
- `user_oauth_auth`
  - Key columns: `user_id`, `oauth_provider`, `subject`
- `user_oidc_links`
  - Purpose: pre-provisioned relationship between a user and the OIDC provider they are allowed to activate against

### Authorization Overlay Tables

- `user_model_allowlist`
  - Relationship: `user_id` + `model_id`
- `team_model_allowlist`
  - Relationship: `team_id` + `model_id`

### Ownership, Accounting, and Logging Tables

- `api_keys`
  - Key columns: `id`, `public_id`, `secret_hash`, `owner_kind`, `owner_user_id`, `owner_team_id`
  - Constraint: exactly one owner column must be set consistently with `owner_kind`
  - Reserved ownership: seeded system-owned keys use the reserved `system-legacy` team
- `user_budgets`
  - Key columns: `user_budget_id`, `user_id`, `cadence`, `amount_10000`, `hard_limit`, `timezone`, `is_active`
  - Constraint: one active user budget per user
- `team_budgets`
  - Key columns: `team_budget_id`, `team_id`, `cadence`, `amount_10000`, `hard_limit`, `timezone`, `is_active`
  - Constraint: one active team budget per team
- `usage_cost_events`
  - Key columns: `usage_event_id`, `request_id`, `ownership_scope_key`, `api_key_id`, `user_id`, `team_id`, `actor_user_id`, `model_id`, `provider_key`, `upstream_model`, `pricing_status`, `unpriced_reason`, `pricing_row_id`, `pricing_provider_id`, `computed_cost_10000`, `provider_usage`, `occurred_at`
  - Notes: this is the canonical spend ledger used for enforcement and reporting
- `request_logs`
  - Key columns: `request_log_id`, `request_id`, `api_key_id`, `user_id`, `team_id`, `model_key`, `resolved_model_key`, `provider_key`, `caller_service`, `caller_component`, `caller_env`, `status_code`, `metadata_json`, `occurred_at`
  - Notes: one summary row per final request outcome
- `request_log_payloads`
  - Key columns: `request_log_id`, `request_json`, `response_json`
  - Notes: summary and payload are intentionally split
- `request_log_tags`
  - Key columns: `request_log_id`, `tag_key`, `tag_value`
  - Notes: bounded bespoke caller tags for request-log filtering and attribution

### Pricing Catalog Cache

- `pricing_catalog_cache`
  - Key columns: `catalog_key`, `source`, `etag`, `fetched_at`, `snapshot_json`
  - Notes: runtime uses the cached snapshot together with the vendored fallback in the repo
- `model_pricing`
  - Key columns: `model_pricing_id`, `pricing_provider_id`, `pricing_model_id`, `effective_start_at`, `effective_end_at`
  - Notes: effective-dated pricing rows are the durable historical charging source
- `usage_cost_event_duplicates_archive`
  - Purpose: preserves duplicate-ledger rows during pricing/ledger migration cleanup and audit backfill flows

## Authorization Semantics

Effective model access is the intersection of:

1. API key grants from `api_key_model_grants`
2. Team allowlist, only when `teams.model_access_mode='restricted'`
3. User allowlist, only when `users.model_access_mode='restricted'`

If neither the team nor the user is restricted, grants remain unchanged.

## Budget and Pricing Notes

- Pricing lookup comes from the internal pricing catalog layer, not from provider `/v1/models` responses
- Supported pricing source ids in this slice are `openai`, `google-vertex`, and `google-vertex-anthropic`
- `openai_compat` providers must declare `pricing_provider_id`
- `gcp_vertex` derives pricing source from the `upstream_model` publisher prefix
- Pricing is exact-only in this slice; unsupported billing modifiers, unsupported publisher/location combinations, and unknown model ids resolve as `unpriced`
- Chargeable requests write usage ledger rows and participate in budget enforcement
- Unpriced requests are not charged and must not be budget-blocked
- Supported budget cadence values are `daily|weekly|monthly`
- Current UTC window semantics:
  - Daily windows start at `00:00:00 UTC`
  - Weekly windows start at `Monday 00:00:00 UTC`
  - Monthly windows start at the first day of the month at `00:00:00 UTC`
  - `Sunday 23:59:59 UTC` remains in the previous weekly window
- Budget threshold alerts persist audit rows plus per-recipient delivery rows and currently deliver owner-only email alerts when remaining budget crosses to `20%` or less
- Team attribution remains `actor:none` today:
  - team key + acting user context attribution is still deferred
  - team key without acting user context is attributed to team only

## Requested vs Resolved Model Identity

- `gateway_models` can either point directly to provider routes or alias another model
- `request_logs.model_key` stores the requested gateway model
- `request_logs.resolved_model_key` stores the canonical execution model after alias resolution

This distinction matters for operator-facing observability and historical debugging. See [model-routing-and-api-behavior.md](../configuration/model-routing-and-api-behavior.md).

## Route Viability Note

Schema alone does not determine whether a model can execute.

Operational viability also depends on:

- provider existence
- route `enabled` state
- positive route weights
- capability filtering

Those rules are owned by [configuration-reference.md](../configuration/configuration-reference.md) and [model-routing-and-api-behavior.md](../configuration/model-routing-and-api-behavior.md).

## Ownership Notes

- User-owned and team-owned API keys share the same `api_keys` table
- Team-owned usage and request logs can exist without an acting user
- Current team spend attribution remains `actor:none` at the ownership-scope level

That ownership model is explained operationally in [identity-and-access.md](../access/identity-and-access.md) and [budgets-and-spending.md](../operations/budgets-and-spending.md).

## PostgreSQL and libsql Parity

Both runtime backends are expected to stay logically aligned for:

- schema shape
- migrations
- seed behavior
- aliases and request-log model identity
- spend ledger behavior
- request-log summary and payload persistence

See [../crates/gateway-store/README.md](../../crates/gateway-store/README.md) for the storage-layer overview.
