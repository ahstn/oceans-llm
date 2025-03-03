# Data Relationships

This document catalogs the database tables, key relationships, and policy semantics used by the identity/user-management foundation.

## Source of Truth

- Schema migrations: `crates/gateway-store/migrations/`
- Identity foundation migration: [`V3__identity_foundation.sql`](/Users/ahstn/git/oceans-llm.feat-ui-init/crates/gateway-store/migrations/V3__identity_foundation.sql)
- Core domain types: [`crates/gateway-core/src/domain.rs`](/Users/ahstn/git/oceans-llm.feat-ui-init/crates/gateway-core/src/domain.rs)
- Repository traits: [`crates/gateway-core/src/traits.rs`](/Users/ahstn/git/oceans-llm.feat-ui-init/crates/gateway-core/src/traits.rs)
- Store implementation: [`crates/gateway-store/src/libsql_store.rs`](/Users/ahstn/git/oceans-llm.feat-ui-init/crates/gateway-store/src/libsql_store.rs)
- Auth/model/budget/logging behavior:
  - [`crates/gateway-service/src/authenticator.rs`](/Users/ahstn/git/oceans-llm.feat-ui-init/crates/gateway-service/src/authenticator.rs)
  - [`crates/gateway-service/src/model_access.rs`](/Users/ahstn/git/oceans-llm.feat-ui-init/crates/gateway-service/src/model_access.rs)
  - [`crates/gateway-service/src/budget_guard.rs`](/Users/ahstn/git/oceans-llm.feat-ui-init/crates/gateway-service/src/budget_guard.rs)
  - [`crates/gateway-service/src/request_logging.rs`](/Users/ahstn/git/oceans-llm.feat-ui-init/crates/gateway-service/src/request_logging.rs)

## Core Entity Graph

1. `teams` 1..N `team_memberships`
2. `users` 0..1 `team_memberships` (unique `user_id` in membership table)
3. `api_keys` belongs to exactly one owner:
   `owner_kind='user'` => `owner_user_id` set, `owner_team_id` null
   `owner_kind='team'` => `owner_team_id` set, `owner_user_id` null
4. `api_keys` N..N `gateway_models` through `api_key_model_grants`
5. Optional restriction overlays:
   - `users` N..N `gateway_models` through `user_model_allowlist`
   - `teams` N..N `gateway_models` through `team_model_allowlist`
6. `users` 1..N `user_budgets` (with one active budget enforced by partial unique index)
7. `usage_cost_events` references request ownership (`api_key_id`, optional `user_id`/`team_id`) and optional `model_id`
8. `request_logs` references request ownership (`api_key_id`, optional `user_id`/`team_id`)

## Table Catalog

### Existing Foundation Tables

- `providers`: upstream provider config and secret references.
- `gateway_models`: gateway model alias registry.
- `model_routes`: route targets per gateway model.
- `api_key_model_grants`: API-key grants to gateway models.
- `audit_logs`: control-plane audit baseline.

### Identity and Auth Tables

- `teams`
  - Key columns: `team_id`, `team_key`, `status`, `model_access_mode`
  - Notes: `model_access_mode` is `all|restricted`.
- `users`
  - Key columns: `user_id`, `name`, `email`, `email_normalized`, `global_role`, `auth_mode`, `request_logging_enabled`, `model_access_mode`
  - Notes: case-insensitive uniqueness enforced via `email_normalized`.
- `team_memberships`
  - Key columns: `team_id`, `user_id`, `role`
  - Notes: one-team-per-user enforced by unique `user_id`.
- `oidc_providers`
  - Key columns: `oidc_provider_id`, `provider_key`, `provider_type`, `issuer_url`, `client_id`, `client_secret_ref`, `scopes_json`, `enabled`
  - Notes: supports `okta|generic_oidc`.
- `user_password_auth`
  - Key columns: `user_id`, `password_hash`, `password_updated_at`
- `user_oidc_auth`
  - Key columns: `user_id`, `oidc_provider_id`, `subject`, `email_claim`
  - Notes: unique `(oidc_provider_id, subject)`.
- `user_oauth_auth`
  - Key columns: `user_id`, `oauth_provider`, `subject`
  - Notes: unique `(oauth_provider, subject)`.

### Authorization Overlay Tables

- `user_model_allowlist`
  - Relationship: `user_id` + `model_id`.
- `team_model_allowlist`
  - Relationship: `team_id` + `model_id`.

### Ownership and Budget Tables

- `api_keys` (single table for both user/team keys)
  - Key columns: `id`, `public_id`, `secret_hash`, `owner_kind`, `owner_user_id`, `owner_team_id`, status fields.
  - Constraint: exactly one owner column set and consistent with `owner_kind`.
  - Migration/backfill: legacy keys are assigned to reserved `system-legacy` team (`00000000-0000-0000-0000-000000000001`).
- `user_budgets`
  - Key columns: `user_budget_id`, `user_id`, `cadence`, `amount_usd`, `hard_limit`, `timezone`, `is_active`
  - Constraint: one active budget per user (`WHERE is_active=1` unique index).
- `usage_cost_events`
  - Key columns: `usage_event_id`, `request_id`, `api_key_id`, `user_id`, `team_id`, `model_id`, `estimated_cost_usd`, `occurred_at`
  - Notes: used for budget accounting regardless of request logging toggle.
- `request_logs`
  - Key columns: `request_log_id`, `request_id`, `api_key_id`, `user_id`, `team_id`, `model_key`, `provider_key`, token/latency/status fields, `metadata_json`, `occurred_at`
  - Notes: user-owned requests honor `users.request_logging_enabled`; team-owned requests are always logged with nullable `user_id`.

## Authorization Semantics

Effective model access is an intersection:

1. API key grants (`api_key_model_grants`)
2. Team allowlist, only if `teams.model_access_mode='restricted'`
3. User allowlist, only if `users.model_access_mode='restricted'`

If no restriction mode applies, grants remain unchanged.

## Budget Semantics

- Budget target: user-owned requests only in this phase.
- Team-owned keys are not budget-blocked in this phase.
- Cadence: `daily|weekly`.
- Enforcement: hard block when projected spend exceeds `amount_usd` and `hard_limit=true`.
- Accounting: `usage_cost_events` are written even when request logging is disabled.

## PostgreSQL Mapping Notes

Current runtime is SQLite/libsql. For PostgreSQL migration/dual-write planning:

1. Keep snake_case naming and equivalent FK/index strategy.
2. For `request_logs`, use:
   - BRIN index on `occurred_at` for large append-heavy scans.
   - Additional btree indexes for frequent filters (for example `(user_id, occurred_at)`, `(team_id, occurred_at)`).
3. For monetary values (`amount_usd`, `estimated_cost_usd`), prefer `NUMERIC` in PostgreSQL.
4. For timestamps, prefer `TIMESTAMPTZ` in PostgreSQL.
