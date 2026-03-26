# ADR: Identity Foundation for Users, Teams, and API Key Ownership

- Date: 2026-03-06
- Status: Accepted

## Implemented By

- Canonical docs:
  - [../identity-and-access.md](../identity-and-access.md)
  - [../data-relationships.md](../data-relationships.md)
  - [../admin-control-plane.md](../admin-control-plane.md)

## Context

The original gateway foundation treated API keys as the primary identity boundary. That was enough for seeded model access, but it did not give us a durable way to represent:

- user-owned versus team-owned credentials,
- future auth methods such as password, OIDC, and OAuth,
- policy overlays above raw API-key grants,
- request logging preferences,
- future budget and spend-accounting state.

PR #5 (`feat/identity-management`, merged into `main` on 2026-03-06) introduced that foundation. Since runtime behavior has evolved after the merge, this ADR records the stable architectural decisions from that work as they exist in the current implementation. In particular, later pricing/catalog work kept the schema groundwork for budgets and spend accounting, but deferred live budget enforcement and `usage_cost_events` writes.

## Decision

### 1. Represent identity with first-class `users`, `teams`, and `team_memberships`

We added explicit identity tables instead of encoding user/team concepts indirectly through API keys or provider config.

Why:
- keeps human identity separate from API credentials,
- gives us stable records for auth mode, role, logging preference, and policy state,
- makes team scoping explicit in the data model.

The current foundation also enforces one team per user through a unique `team_memberships.user_id`.

Why:
- matches the current product scope,
- keeps membership lookups and effective-policy resolution simple,
- can be relaxed later with an intentional migration if multi-team membership becomes necessary.

### 2. Keep API key ownership in one table with explicit owner metadata

We kept a single `api_keys` table and extended it with:

- `owner_kind`
- `owner_user_id`
- `owner_team_id`

with database constraints ensuring exactly one owner is set and that it matches `owner_kind`.

Why:
- the auth path stays uniform for both user-owned and team-owned keys,
- ownership can be validated at the database boundary instead of only in service code,
- downstream authz, logging, and future accounting can all read from one ownership model.

### 3. Preserve backward compatibility with a reserved `system-legacy` team

Legacy and seeded keys are backfilled to a reserved team (`system-legacy`) during migration and seeding.

Why:
- avoids breaking existing deployments and config-driven seed data,
- gives all pre-identity keys a valid owner without inventing synthetic users,
- lets the system move to ownership-aware auth without forcing immediate operator cleanup.

### 4. Model authorization is layered as grants plus optional user/team restrictions

We kept `api_key_model_grants` as the baseline access list and added:

- `teams.model_access_mode`
- `users.model_access_mode`
- `team_model_allowlist`
- `user_model_allowlist`

Effective access is the intersection of:

1. API key grants
2. Team allowlist when the team is `restricted`
3. User allowlist when the user is `restricted`

Why:
- preserves seeded API-key grants as the baseline contract,
- allows team- and user-level restriction without duplicating grant rows,
- supports an `all` mode that adds no extra overlay complexity.

### 5. Standardize identity and policy state as explicit domain enums

We introduced typed domain values for:

- `AuthMode`
- `GlobalRole`
- `MembershipRole`
- `ModelAccessMode`
- `BudgetCadence`
- `ApiKeyOwnerKind`

Why:
- removes stringly typed policy logic from the service layer,
- makes serialization and migration rules explicit,
- keeps repository decoding strict when persisted values drift from expected state.

### 6. Separate operational request logging from future spend accounting

The foundation added both `request_logs` and the schema groundwork for `user_budgets` and `usage_cost_events`, but they are intentionally not the same concern.

Current runtime behavior:
- executed chat requests may write `request_logs`,
- user-owned requests honor `users.request_logging_enabled`,
- team-owned requests always log with nullable `user_id`,
- `/v1/chat/completions` does not currently enforce budgets,
- `/v1/chat/completions` does not currently write `usage_cost_events`.

Why:
- request logging is useful operationally before pricing/accounting is complete,
- spend accounting must not go live until attribution and pricing rules are exact,
- keeping the schema in place now avoids later churn when budget work resumes.

### 7. Store persisted money values as exact fixed-point integers

We introduced `Money4` in the core domain and store persisted money as scaled integers (`*_10000`) rather than floating-point values.

Why:
- budget and pricing calculations need deterministic, exact arithmetic,
- floating-point storage would create drift in later accounting flows,
- the migration path is explicit and reversible at the schema level.

### 8. Keep this slice data-first, not a full identity product rollout

The accepted foundation scope is:

- schema and migration support,
- ownership-aware authentication metadata,
- policy-aware model authorization,
- request logging controls,
- budget and spend-accounting schema groundwork.

It does not yet include:

- end-user login flows,
- admin CRUD for users/teams/memberships,
- live budget enforcement on chat requests,
- live `usage_cost_events` writes,
- acting-user attribution for team-key spend accounting.

Why:
- durable data model decisions needed to land before product workflows,
- foundational migrations are harder to change later than handler-level behavior,
- this keeps future identity and spend work additive instead of schema-destructive.

## Consequences

Positive:
- the gateway now has a durable ownership model for users, teams, and API keys,
- legacy keys remain compatible through the reserved `system-legacy` team,
- model authorization has a clear, layered policy model,
- request logging and future spend/accounting work can evolve without redesigning the base schema,
- persisted money values are now safe for exact future accounting.

Tradeoffs:
- the current membership model is intentionally restrictive (`0..1` team per user),
- several tables introduced in this slice are groundwork and not yet on the live spend path,
- ownership-aware auth and authz add cross-table lookups that did not exist in the original key-only model.

## Follow-up Work

- Add control-plane CRUD and admin workflows for users, teams, memberships, and allowlists.
- Introduce explicit acting-user context if team-key request attribution must include a concrete user identity.
- Re-enable pricing-backed budget enforcement only with exact pricing coverage and idempotent spend writes.
- Revisit the one-team-per-user constraint if product requirements move to multi-team membership.

## Attribution

This ADR retrospectively documents merged PR #5 (`feat/identity-management`) in the context of the current implementation. It was prepared through collaborative human + AI implementation/design work.
