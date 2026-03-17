# Data Relationships

`Owns`: schema-level entities, table relationships, ownership boundaries, and cross-table invariants.
`Depends on`: [identity-and-access.md](identity-and-access.md), [model-routing-and-api-behavior.md](model-routing-and-api-behavior.md)
`See also`: [budgets-and-spending.md](budgets-and-spending.md), [observability-and-request-logs.md](observability-and-request-logs.md), [adr/2026-03-05-identity-foundation.md](adr/2026-03-05-identity-foundation.md)

This document is schema-oriented. It describes the persistent relationships that are hard to infer from a single file, but it does not try to restate every runtime rule owned by neighboring docs.

## Source of Truth

- Migrations:
  - [../crates/gateway-store/migrations/](../crates/gateway-store/migrations/)
  - [../crates/gateway-store/migrations/postgres/](../crates/gateway-store/migrations/postgres/)
- Core types:
  - [../crates/gateway-core/src/domain.rs](../crates/gateway-core/src/domain.rs)
  - [../crates/gateway-core/src/traits.rs](../crates/gateway-core/src/traits.rs)
- Runtime behavior:
  - [../crates/gateway-service/src/model_access.rs](../crates/gateway-service/src/model_access.rs)
  - [../crates/gateway-service/src/model_resolution.rs](../crates/gateway-service/src/model_resolution.rs)
  - [../crates/gateway-service/src/request_logging.rs](../crates/gateway-service/src/request_logging.rs)
  - [../crates/gateway-service/src/budget_guard.rs](../crates/gateway-service/src/budget_guard.rs)

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

## Table Catalog

### Foundation Tables

- `providers`: upstream provider config and secret references
- `gateway_models`: gateway model registry; rows can be provider-backed or alias-backed
- `model_routes`: execution targets for provider-backed models only
- `api_key_model_grants`: model grants attached to an API key
- `audit_logs`: control-plane audit baseline

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
  - Backfill rule: legacy keys are assigned to the reserved `system-legacy` team
- `user_budgets`
  - Key columns: `user_budget_id`, `user_id`, `cadence`, `amount_10000`, `hard_limit`, `timezone`, `is_active`
  - Constraint: one active user budget per user
- `team_budgets`
  - Key columns: `team_budget_id`, `team_id`, `cadence`, `amount_10000`, `hard_limit`, `timezone`, `is_active`
  - Constraint: one active team budget per team
- `usage_cost_events`
  - Key columns: `usage_event_id`, `request_id`, `ownership_scope_key`, `api_key_id`, `user_id`, `team_id`, `model_id`, `pricing_status`, `computed_cost_10000`, `occurred_at`
  - Notes: this is the canonical spend ledger used for enforcement and reporting
- `request_logs`
  - Key columns: `request_log_id`, `request_id`, `api_key_id`, `user_id`, `team_id`, `model_key`, `resolved_model_key`, `provider_key`, `status_code`, `metadata_json`, `occurred_at`
  - Notes: one summary row per final request outcome
- `request_log_payloads`
  - Key columns: `request_log_id`, `request_json`, `response_json`
  - Notes: summary and payload are intentionally split

### Pricing Catalog Cache

- `pricing_catalog_cache`
  - Key columns: `catalog_key`, `source`, `etag`, `fetched_at`, `snapshot_json`
  - Notes: runtime uses the cached snapshot together with the vendored fallback in the repo

## Authorization Semantics

Effective model access is the intersection of:

1. API key grants from `api_key_model_grants`
2. Team allowlist, only when `teams.model_access_mode='restricted'`
3. User allowlist, only when `users.model_access_mode='restricted'`

If neither the team nor the user is restricted, grants remain unchanged.

## Requested vs Resolved Model Identity

- `gateway_models` can either point directly to provider routes or alias another model
- `request_logs.model_key` stores the requested gateway model
- `request_logs.resolved_model_key` stores the canonical execution model after alias resolution

This distinction matters for operator-facing observability and historical debugging. See [model-routing-and-api-behavior.md](model-routing-and-api-behavior.md).

## Ownership Notes

- User-owned and team-owned API keys share the same `api_keys` table
- Team-owned usage and request logs can exist without an acting user
- Current team spend attribution remains `actor:none` at the ownership-scope level

That ownership model is explained operationally in [identity-and-access.md](identity-and-access.md) and [budgets-and-spending.md](budgets-and-spending.md).

## PostgreSQL and libsql Parity

Both runtime backends are expected to stay logically aligned for:

- schema shape
- migrations
- seed behavior
- aliases and request-log model identity
- spend ledger behavior
- request-log summary and payload persistence

See [../crates/gateway-store/README.md](../crates/gateway-store/README.md) for the storage-layer overview.
