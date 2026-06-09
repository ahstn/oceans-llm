# Data Relationships

`See also`: [Identity and Access](../access/identity-and-access.md), [Service Accounts](../access/service-accounts.md), [Model Routing and API Behavior](../configuration/model-routing-and-api-behavior.md), [Provider API Compatibility](provider-api-compatibility.md), [Budgets and Spending](../operations/budgets-and-spending.md), [Observability and Request Logs](../operations/observability-and-request-logs.md), [Request Logs](../operations/observability/request-logs.md), [MCP Invocations](../operations/observability/mcp-invocations.md), [MCP Registry and Discovery](../operations/observability/mcp-registry-and-discovery.md), [ADR: Team Service Accounts for Non-Human Gateway Access](../adr/2026-05-10-team-service-accounts.md), [ADR: Identity Foundation for Users, Teams, and API Key Ownership](../adr/2026-03-05-identity-foundation.md), [ADR: Route-Level Provider API Compatibility Profiles](../adr/2026-04-23-route-level-provider-api-compatibility-profiles.md), [ADR: MCP Tool Cardinality Observability](../adr/2026-04-28-mcp-tool-cardinality-observability.md), [ADR: External MCP Registry and Discovery Boundary](../adr/2026-05-26-external-mcp-registry-and-discovery.md)

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

1. `teams` 0..N `team_memberships`
2. `users` 0..1 `team_memberships`
3. `teams` 0..N `service_accounts`
4. `api_keys` belongs to exactly one principal owner: user or service account
5. `api_keys` N..N `gateway_models` through `api_key_model_grants`
6. Optional restriction overlays:
   - `user_model_allowlist`
   - `team_model_allowlist`
7. `budgets` stores user, service-account, and user-model budgets with one active row per canonical scope key
8. `usage_cost_events` records request ownership, model attribution, pricing status, and computed cost
9. `request_logs` records the final user-visible request outcome
10. `request_log_payloads` stores sanitized request and response bodies separately from the summary row
11. `request_log_attempts` stores ordered upstream provider execution attempts for a request log
12. `mcp_tool_invocations` stores individual tool-call audit rows correlated by `request_id`
13. Request-log purge removes old request-log parents and their payload, tag, and attempt children without touching spend ledger rows
14. `pricing_catalog_cache` stores normalized pricing snapshots used by runtime pricing resolution
15. `model_pricing` stores effective-dated pricing rows used for historical charging
16. `usage_cost_event_duplicates_archive` preserves duplicate-ledger migration/archive context
17. `external_mcp_servers` stores user-added MCP server registry rows and soft-disable state
18. `external_mcp_tools` stores discovered MCP tools, stable tool ids, schema hashes, schema versions, and active/inactive state
19. `external_mcp_discovery_runs` stores immutable discovery attempt diagnostics

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
- `service_accounts`
  - Key columns: `service_account_id`, `team_id`, `name`, `status`, `created_by_user_id`, `deactivated_at`
  - Notes: service accounts are team-owned non-human principals; deletion is deactivation
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
  - Key columns: `id`, `public_id`, `secret_hash`, `owner_kind`, `owner_user_id`, `owner_service_account_id`
  - Constraint: exactly one owner column must be set consistently with `owner_kind`
  - Notes: direct team-owned runtime keys and `system-legacy` ownership are not supported
- `budgets`
  - Key columns: `budget_id`, `scope_kind`, `scope_key`, `user_id`, `service_account_id`, `model_id`, `upstream_model`, `cadence`, `amount_10000`, `hard_limit`, `timezone`, `is_active`
  - Constraint: one active budget per canonical `scope_key`
  - Notes: supported scope kinds are `user`, `service_account`, and `user_model`; teams are not budget principals
- `usage_cost_events`
  - Key columns: `usage_event_id`, `request_id`, `ownership_scope_key`, `api_key_id`, `user_id`, `team_id`, `actor_user_id`, `model_id`, `provider_key`, `upstream_model`, `pricing_status`, `unpriced_reason`, `pricing_row_id`, `pricing_provider_id`, `computed_cost_10000`, `provider_usage`, `occurred_at`
  - Notes: this is the canonical spend ledger used for enforcement and reporting
- `request_logs`
  - Key columns: `request_log_id`, `request_id`, `api_key_id`, `user_id`, `team_id`, `model_key`, `resolved_model_key`, `provider_key`, `caller_service`, `caller_component`, `caller_env`, `status_code`, `referenced_mcp_server_count`, `exposed_tool_count`, `invoked_tool_count`, `filtered_tool_count`, `user_agent_raw`, `agent_harness_key`, `agent_harness_label`, `metadata_json`, `occurred_at`
  - Notes: one summary row per final request outcome; `metadata_json.payload_policy` records the capture mode and limits used for the row when request logging is enabled; tool-cardinality columns are nullable typed facts, with historical or not-yet-observable dimensions left `null`; agent harness usage groups by `agent_harness_key` while preserving bounded raw `User-Agent` detail evidence
- `request_log_payloads`
  - Key columns: `request_log_id`, `request_json`, `response_json`
  - Notes: summary and payload are intentionally split; rows exist only when the payload policy captures redacted payloads
- `request_log_tags`
  - Key columns: `request_log_id`, `tag_key`, `tag_value`
  - Notes: bounded bespoke caller tags for request-log filtering and attribution
- `request_log_attempts`
  - Key columns: `request_attempt_id`, `request_log_id`, `request_id`, `attempt_number`, `route_id`, `provider_key`, `upstream_model`, `status`, `retryable`, `terminal`, `produced_final_response`, `stream`, `started_at`, `completed_at`, `latency_ms`
  - Notes: attempt rows are metadata-only children of `request_logs`; they are ordered by `attempt_number` and describe provider execution, not pre-provider gateway rejections
- `mcp_tool_invocations`
  - Key columns: `mcp_tool_invocation_id`, `request_log_id`, `request_id`, `api_key_id`, `user_id`, `team_id`, `owner_kind`, `server_id`, `server_display_key`, `tool_id`, `tool_display_key`, `status`, `policy_result`, `latency_ms`, `error_code`, `has_payload`, `arguments_payload_truncated`, `result_payload_truncated`, `arguments_payload_redacted`, `result_payload_redacted`, `metadata_json`, `occurred_at`
  - Notes: MCP invocation rows are correlated by `request_id`; `request_log_id` is nullable and non-owning because request-log summaries are written at final outcome and may be absent or purged independently. Server/tool stable IDs are nullable until registry records exist, but display keys are required.
- `mcp_tool_invocation_payloads`
  - Key columns: `mcp_tool_invocation_id`, `arguments_json`, `result_json`
  - Notes: payload rows exist only when MCP invocation payload policy captures sanitized payloads; summary rows are still recorded when payload capture is disabled.
- `mcp_aggregate_sessions`
  - Key columns: `session_id`, `token_hash`, `api_key_id`, `owner_kind`, `owner_user_id`, `owner_team_id`, `owner_service_account_id`, `protocol_version`, `initialized`, `expires_at`, `created_at`, `updated_at`, `revoked_at`
  - Notes: aggregate `/mcp` Streamable HTTP sessions are durable transport state. Only token hashes are stored. Sessions are bound to the authenticated API key and owner metadata; reuse by another principal is treated as not found.

### External MCP Registry Tables

- `external_mcp_servers`
  - Key columns: `mcp_server_id`, `server_key`, `display_name`, `transport`, `server_url`, `auth_mode`, `auth_config_json`, `timeout_ms`, `status`, `last_discovery_status`, `last_discovery_at`, `last_successful_discovery_at`, `last_error_summary`, `last_tool_count`, `created_at`, `updated_at`, `disabled_at`
  - Notes: rows are user-added registry records. Recommended catalog entries are not seeded into this table. Disabling a server is the archive/delete path; hard delete is not exposed.
- `external_mcp_tools`
  - Key columns: `mcp_tool_id`, `mcp_server_id`, `upstream_name`, `display_name`, `description`, `input_schema_json`, `schema_hash`, `schema_version`, `is_active`, `first_discovered_at`, `last_discovered_at`, `deactivated_at`
  - Notes: `(mcp_server_id, upstream_name)` is unique. Rediscovery preserves `mcp_tool_id` for stable upstream names and increments `schema_version` only when `schema_hash` changes.
- `external_mcp_discovery_runs`
  - Key columns: `discovery_run_id`, `mcp_server_id`, `status`, `started_at`, `finished_at`, `discovered_tool_count`, `active_tool_count`, `schema_set_hash`, `error_summary`, `details_json`
  - Notes: diagnostics are bounded and must not contain raw tokens or user credentials.

Request-log purge treats `request_logs` as the parent retention boundary. Purging a parent row also removes matching `request_log_payloads`, `request_log_tags`, and `request_log_attempts` rows. The purge does not remove `usage_cost_events`; spend reporting and budget enforcement stay tied to the ledger.

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

Service-account credentials use API-key grants plus the owning team's allowlist. User allowlists do not apply to service accounts.

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
- Service-account budget alerts are delivered to active owners and admins of the owning team
- Service-account spend remains attributable to the service account and its owning team

## Requested vs Resolved Model Identity

- `gateway_models` can either point directly to provider routes or alias another model
- `request_logs.model_key` stores the requested gateway model
- `request_logs.resolved_model_key` stores the canonical execution model after alias resolution

This distinction matters for admin-facing observability and historical debugging. See [model-routing-and-api-behavior.md](../configuration/model-routing-and-api-behavior.md).

## Route Viability Note

Schema alone does not determine whether a model can execute.

Operational viability also depends on:

- provider existence
- route `enabled` state
- positive route weights
- capability filtering

Those rules are owned by [configuration-reference.md](../configuration/configuration-reference.md) and [model-routing-and-api-behavior.md](../configuration/model-routing-and-api-behavior.md).

## Ownership Notes

- User-owned and service-account-owned API keys share the same `api_keys` table
- Service-account usage and request logs exist without an acting user
- Direct team-owned runtime API keys are removed from the schema contract
- There is no reserved `system-legacy` ownership path

That ownership model is explained operationally in [identity-and-access.md](../access/identity-and-access.md), [service-accounts.md](../access/service-accounts.md), and [budgets-and-spending.md](../operations/budgets-and-spending.md).

## PostgreSQL and libsql Parity

Both runtime backends are expected to stay logically aligned for:

- schema shape
- migrations
- seed behavior
- aliases and request-log model identity
- spend ledger behavior
- request-log summary, payload, tag, and attempt persistence

See [../crates/gateway-store/README.md](../../crates/gateway-store/README.md) for the storage-layer overview.
