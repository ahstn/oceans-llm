# ADR: Declarative Config-Seeded Identity and Budget Reconciliation

- Date: 2026-03-31
- Status: Accepted
- Related Issues:
  - [#64](https://github.com/ahstn/oceans-llm/issues/64)
  - [#65](https://github.com/ahstn/oceans-llm/issues/65)
  - [#29](https://github.com/ahstn/oceans-llm/issues/29)
  - [#46](https://github.com/ahstn/oceans-llm/issues/46)
- Builds On:
  - [2026-03-05-identity-foundation.md](2026-03-05-identity-foundation.md)
  - [2026-03-15-spend-control-plane-reporting-and-team-hard-limits.md](2026-03-15-spend-control-plane-reporting-and-team-hard-limits.md)
  - [2026-03-26-admin-identity-lifecycle-and-team-member-workflows.md](2026-03-26-admin-identity-lifecycle-and-team-member-workflows.md)
  - [2026-03-28-generated-admin-api-contract-and-typed-same-origin-client.md](2026-03-28-generated-admin-api-contract-and-typed-same-origin-client.md)

## Current state

- [../configuration/configuration-reference.md](../configuration/configuration-reference.md)
- [../access/identity-and-access.md](../access/identity-and-access.md)
- [../operations/budgets-and-spending.md](../operations/budgets-and-spending.md)
- [../setup/runtime-bootstrap-and-access.md](../setup/runtime-bootstrap-and-access.md)

## Context

Before this change, startup configuration could seed providers, models, and gateway API keys, but it could not declare the human identity and spend objects that the rest of the product already depended on.

That left an awkward split:

- runtime bootstrap was config-driven for model routing and data-plane access,
- identity and budget ownership were database-backed and admin-managed,
- deploy examples could create API access, but not the team, user, and budget state needed to operate the control plane coherently.

Issue [#64](https://github.com/ahstn/oceans-llm/issues/64) asked for declarative teams and users in `gateway.yaml`. The request matters because the surrounding architecture was already in place:

- identity records and membership rules already existed in [../../crates/gateway-store/src/libsql_store/identity.rs](../../crates/gateway-store/src/libsql_store/identity.rs) and [../../crates/gateway-store/src/postgres_store/identity.rs](../../crates/gateway-store/src/postgres_store/identity.rs),
- budget upsert and deactivate behavior already existed in [../../crates/gateway-store/src/libsql_store/budgets.rs](../../crates/gateway-store/src/libsql_store/budgets.rs) and [../../crates/gateway-store/src/postgres_store/budgets.rs](../../crates/gateway-store/src/postgres_store/budgets.rs),
- startup seeding was already centralized in [../../crates/gateway/src/main.rs](../../crates/gateway/src/main.rs),
- the admin lifecycle rules already constrained auth-mode changes, owner memberships, and user status in [../../crates/gateway/src/http/identity_lifecycle.rs](../../crates/gateway/src/http/identity_lifecycle.rs).

The risk was not lack of primitives. The risk was introducing a second bootstrap path for identity, budgets, or deploy defaults and letting it drift from the existing store and admin behavior.

## Decision

We extend the existing config seed contract to reconcile teams, users, memberships, OIDC links, and active budgets through the same store-owned seeding path already used for providers, models, and API keys.

The key decisions are:

### 1. Keep one seed pipeline instead of adding an identity-only bootstrap path

The canonical seed path still starts in [../../crates/gateway/src/main.rs](../../crates/gateway/src/main.rs) and flows through `GatewayStore::seed_from_inputs` in [../../crates/gateway-store/src/store.rs](../../crates/gateway-store/src/store.rs).

Why:

- startup policy should stay centralized,
- backend-specific reconciliation belongs in the store layer, not in `main.rs`,
- one seed contract is easier to reason about and test than parallel seeders with overlapping responsibility.

### 2. Make declarative identity config desired-state for listed entries, not global pruning

The config now supports top-level `teams` and `users` in [../../crates/gateway/src/config.rs](../../crates/gateway/src/config.rs), but the reconciliation scope is intentionally narrow:

- listed teams are authoritative for their mutable fields and active budget,
- listed users are authoritative for their mutable fields, membership, and active budget,
- unlisted teams and users are left untouched.

Why:

- the gateway already has live admin-managed identity state,
- deleting unlisted rows would make config application dangerously destructive,
- desired-state behavior is still useful when it is scoped to the entries the config explicitly owns.

### 3. Key teams by `team_key` and users by normalized email

Seeded teams reconcile on `team_key`, while seeded users reconcile on normalized email.

Why:

- those identifiers already match the durable product model,
- `team_key` is the stable external reference for config,
- normalized email is the most stable pre-auth identity handle available in the current product.

### 4. Reuse existing lifecycle and membership constraints rather than bypass them

The seed path deliberately follows the same policy boundaries as the admin lifecycle:

- new config-seeded users are created as `invited`,
- auth mode can only change for already-invited users,
- `owner` memberships cannot be created, removed, or transferred,
- one team per user remains the live constraint,
- OIDC config supports pre-provisioned provider links but not hardened claims-driven identity matching.

Why:

- config seeding should not be a privilege escalation path around the admin rules,
- lifecycle consistency matters more than seed-time convenience,
- issue [#65](https://github.com/ahstn/oceans-llm/issues/65) remains the proper place for hardened declarative SSO-backed identity behavior.

### 5. Treat config budgets as ownership of the active budget row only

Seeded budgets map to the active user or team budget and use the existing upsert/deactivate primitives. Historical budget rows remain historical.

Why:

- config should describe live spend policy, not rewrite financial history,
- the existing budget schema already distinguishes active versus inactive rows,
- this keeps declarative budgets compatible with the existing reporting and alerting model.

### 6. Remove stale config examples instead of preserving old field names

This change also corrects checked-in config and docs to use the real current contract:

- `auth.seed_api_keys[*].value`, not `key`,
- Vertex `credentials_path`, not `service_account_json`,
- bootstrap-admin password secret references that match the parser,
- deploy examples that declare bootstrap admin, teams, users, and budgets directly instead of relying on undocumented manual follow-up.

Why:

- stale examples are a form of compatibility shim,
- incorrect examples teach the wrong contract and create operational drift,
- this project is better served by one accurate path than by preserving legacy wording in docs.

## Implementation

### Config and seed DTOs

- [../../crates/gateway/src/config.rs](../../crates/gateway/src/config.rs)
  - adds `teams` and `users`,
  - validates reserved identifiers, duplicate emails, membership rules, and budget config,
  - emits `seed_teams()` and `seed_users()`.
- [../../crates/gateway-core/src/domain.rs](../../crates/gateway-core/src/domain.rs)
  - adds `SeedBudget`, `SeedTeam`, `SeedUserMembership`, and `SeedUser`.
- [../../crates/gateway-core/src/lib.rs](../../crates/gateway-core/src/lib.rs)
  - re-exports the new seed DTOs.

This keeps the seed contract explicit and transport-neutral before it reaches the store layer.

### Shared store reconciliation policy

- [../../crates/gateway-store/src/store.rs](../../crates/gateway-store/src/store.rs)
  - extends the store contract with declarative seed inputs and a seed-only profile update primitive.
- [../../crates/gateway-store/src/seed.rs](../../crates/gateway-store/src/seed.rs)
  - centralizes team and user reconciliation policy shared by both backends.

This is the critical structural choice in the implementation. We did not duplicate the policy in the libsql and PostgreSQL stores. Both backends share the same reconciliation rules and only diverge where they need backend-specific SQL for profile updates.

### Backend-specific seed execution

- [../../crates/gateway-store/src/libsql_store/seed.rs](../../crates/gateway-store/src/libsql_store/seed.rs)
- [../../crates/gateway-store/src/postgres_store/seed.rs](../../crates/gateway-store/src/postgres_store/seed.rs)
- [../../crates/gateway-store/src/libsql_store/mod.rs](../../crates/gateway-store/src/libsql_store/mod.rs)
- [../../crates/gateway-store/src/postgres_store/mod.rs](../../crates/gateway-store/src/postgres_store/mod.rs)

Those files now:

- preserve the existing provider/model/API-key seeding behavior,
- call the shared reconciliation helpers for teams and users,
- update user profile fields that the admin mutation path does not currently own directly.

### Startup integration

- [../../crates/gateway/src/main.rs](../../crates/gateway/src/main.rs)
  - now passes seeded teams and users through the same `seed-config` path used during startup and explicit CLI seeding.

That keeps `gateway serve --seed-config` and `gateway seed-config` behavior aligned.

### Admin visibility

- [../../crates/gateway/src/http/admin_contract.rs](../../crates/gateway/src/http/admin_contract.rs)
- [../../crates/gateway/src/http/identity_views.rs](../../crates/gateway/src/http/identity_views.rs)
- [../../crates/admin-ui/web/src/routes/identity/users.tsx](../../crates/admin-ui/web/src/routes/identity/users.tsx)
- [../../crates/gateway/openapi/admin-api.json](../../crates/gateway/openapi/admin-api.json)
- [../../crates/admin-ui/web/src/generated/admin-api.ts](../../crates/admin-ui/web/src/generated/admin-api.ts)

The admin identity view now exposes `request_logging_enabled` read-only.

Why:

- config can now own that user preference,
- operators need to see the effective seeded state,
- hidden config-owned state creates confusion and weakens the value of config as the source of truth.

### Tests and examples

- [../../crates/gateway-store/src/lib.rs](../../crates/gateway-store/src/lib.rs)
  - adds reseed coverage for libsql and PostgreSQL, including membership transfer, budget activation/deactivation, OIDC linking, and idempotence.
- [../../deploy/config/gateway.yaml](../../deploy/config/gateway.yaml)
- [../../deploy/.env.example](../../deploy/.env.example)
  - now describe the deploy-time identity and bootstrap path directly.

Canonical docs updated with the new contract:

- [../configuration/configuration-reference.md](../configuration/configuration-reference.md)
- [../access/identity-and-access.md](../access/identity-and-access.md)
- [../operations/budgets-and-spending.md](../operations/budgets-and-spending.md)
- [../setup/runtime-bootstrap-and-access.md](../setup/runtime-bootstrap-and-access.md)
- [../access/oidc-and-sso-status.md](../access/oidc-and-sso-status.md)

## Consequences

Positive:

- deploys can now describe the operational identity baseline in one config file,
- teams, users, memberships, and active budgets now converge through the same startup path as the rest of runtime config,
- libsql and PostgreSQL share one reconciliation policy,
- config no longer hides request-logging preference from operators,
- the checked-in docs now match the actual parser and deploy examples.

Tradeoffs:

- config seeding is now responsible for more policy-aware behavior and therefore has a larger test surface,
- the current declarative OIDC path is intentionally limited to pre-provisioned links and does not solve hardened SSO claims policy,
- unlisted entries are intentionally not pruned, so config ownership is scoped rather than absolute over the entire identity graph.

## Follow-up

- Harden declarative SSO-backed identity matching, claims policy, and provider-driven lifecycle behavior in [#65](https://github.com/ahstn/oceans-llm/issues/65).
- Keep local test-IdP guidance and hardened OIDC flow work scoped to [#46](https://github.com/ahstn/oceans-llm/issues/46) and [#29](https://github.com/ahstn/oceans-llm/issues/29) rather than broadening the current config contract prematurely.
- If config eventually needs full pruning semantics, introduce that as an explicit ownership mode rather than making current seed application silently destructive.

## Attribution

This ADR was prepared through collaborative human + AI implementation and documentation work.
