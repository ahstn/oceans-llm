# Data Relationships

This document catalogs the database tables, key relationships, and policy semantics used by the identity/user-management foundation.

## Source of Truth

- Schema migrations: `crates/gateway-store/migrations/`
- Identity foundation migration: [`V3__identity_foundation.sql`](/Users/ahstn/git/oceans-llm.feat-ui-init/crates/gateway-store/migrations/V3__identity_foundation.sql)
- Money precision migration: [`V4__money_fixed_point.sql`](/Users/ahstn/git/oceans-llm.feat-ui-init/crates/gateway-store/migrations/V4__money_fixed_point.sql)
- Core domain types: [`crates/gateway-core/src/domain.rs`](/Users/ahstn/git/oceans-llm.feat-ui-init/crates/gateway-core/src/domain.rs)
- Repository traits: [`crates/gateway-core/src/traits.rs`](/Users/ahstn/git/oceans-llm.feat-ui-init/crates/gateway-core/src/traits.rs)
- Store implementation: [`crates/gateway-store/src/libsql_store.rs`](/Users/ahstn/git/oceans-llm.feat-ui-init/crates/gateway-store/src/libsql_store.rs)
- Auth/model/logging behavior and deferred budget foundation:
  - [`crates/gateway-service/src/authenticator.rs`](/Users/ahstn/git/oceans-llm.feat-ui-init/crates/gateway-service/src/authenticator.rs)
  - [`crates/gateway-service/src/model_access.rs`](/Users/ahstn/git/oceans-llm.feat-ui-init/crates/gateway-service/src/model_access.rs)
  - [`crates/gateway-service/src/budget_guard.rs`](/Users/ahstn/git/oceans-llm.feat-ui-init/crates/gateway-service/src/budget_guard.rs)
  - [`crates/gateway-service/src/request_logging.rs`](/Users/ahstn/git/oceans-llm.feat-ui-init/crates/gateway-service/src/request_logging.rs)
- User-facing policy guide: [`docs/budgets-and-spending.md`](/Users/ahstn/git/oceans-llm.feat-ui-init/docs/budgets-and-spending.md)

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
  - Key columns: `user_budget_id`, `user_id`, `cadence`, `amount_10000`, `hard_limit`, `timezone`, `is_active`
  - Constraint: one active budget per user (`WHERE is_active=1` unique index).
- `usage_cost_events`
  - Key columns: `usage_event_id`, `request_id`, `api_key_id`, `user_id`, `team_id`, `model_id`, `estimated_cost_10000`, `occurred_at`
  - Notes: schema foundation for future pricing-backed spend accounting; the current chat request path does not write these rows yet.
- `request_logs`
  - Key columns: `request_log_id`, `request_id`, `api_key_id`, `user_id`, `team_id`, `model_key`, `provider_key`, token/latency/status fields, `metadata_json`, `occurred_at`
  - Notes: chat execution writes one row for the final user-visible outcome of each executed request. User-owned requests honor `users.request_logging_enabled`; team-owned requests are always logged with nullable `user_id`.

## Authorization Semantics

Effective model access is an intersection:

1. API key grants (`api_key_model_grants`)
2. Team allowlist, only if `teams.model_access_mode='restricted'`
3. User allowlist, only if `users.model_access_mode='restricted'`

If no restriction mode applies, grants remain unchanged.

## Budget Semantics

- `user_budgets` and `usage_cost_events` are present as schema groundwork for later pricing-ledger work.
- Current runtime behavior: `/v1/chat/completions` does not enforce budgets and does not write `usage_cost_events`.
- Planned target when pricing exists: user-owned requests first; team-owned keys remain outside user-budget blocking in the initial rollout.
- Planned cadence values remain `daily|weekly`.
- Planned UTC window semantics:
  - Daily windows start at `00:00:00 UTC`.
  - Weekly windows start at `Monday 00:00:00 UTC`.
  - `Sunday 23:59:59 UTC` is still in the previous weekly window.
- Planned attribution policy once accounting is wired:
  - Team key + acting user context: usage is attributed to both user and team.
  - Team key without acting user context: usage is attributed to team only.
- User-facing behavior details are documented in [`docs/budgets-and-spending.md`](/Users/ahstn/git/oceans-llm.feat-ui-init/docs/budgets-and-spending.md).

## PostgreSQL Mapping Notes

Current runtime is SQLite/libsql. For PostgreSQL migration/dual-write planning:

1. Keep snake_case naming and equivalent FK/index strategy.
2. For `request_logs`, use:
   - BRIN index on `occurred_at` for large append-heavy scans.
   - Additional btree indexes for frequent filters (for example `(user_id, occurred_at)`, `(team_id, occurred_at)`).
3. For monetary values, SQLite stores exact scaled integers (`amount_10000`, `estimated_cost_10000`).
4. PostgreSQL mapping should use `NUMERIC(18,4)` for these values.
5. For timestamps, prefer `TIMESTAMPTZ` in PostgreSQL.
