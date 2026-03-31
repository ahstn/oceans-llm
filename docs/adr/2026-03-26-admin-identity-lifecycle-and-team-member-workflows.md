# ADR: Admin Identity Lifecycle and Team Member Workflow Hardening

- Date: 2026-03-26
- Status: Accepted
- Related Issues:
  - [#32](https://github.com/ahstn/oceans-llm/issues/32)
  - [#33](https://github.com/ahstn/oceans-llm/issues/33)
- Builds On:
  - [2026-03-05-identity-foundation.md](2026-03-05-identity-foundation.md)
  - [2026-03-08-admin-team-management-flow.md](2026-03-08-admin-team-management-flow.md)

## Current state

- [../identity-and-access.md](../access/identity-and-access.md)
- [../admin-control-plane.md](../access/admin-control-plane.md)

## Context

The earlier identity and team-management slices established the basic control-plane shape:

- users could be created and onboarded,
- teams could be created and administered,
- one user could belong to at most one team,
- membership roles already carried stronger semantics than simple UI labels.

That foundation was enough to bootstrap the product, but it was still missing the operational lifecycle needed to run the system over time.

In practice, platform admins need to do more than create users:

- correct a mistaken global role or team assignment,
- disable a user immediately without deleting audit history,
- reactivate a user later,
- restart onboarding when password or OIDC setup gets stuck,
- remove someone from a team,
- transfer someone between teams without inventing manual SQL or unsafe side effects.

Those actions are not ordinary CRUD. They change authorization boundaries, active sessions, onboarding state, and the future interpretation of team-derived access. Leaving that behavior implicit or UI-only would create several failure modes:

- disabled users could retain stale session access,
- auth-mode changes could orphan existing credentials,
- the last active platform admin could remove their own control-plane access,
- team transfer could accidentally behave like historical ownership migration,
- store implementations could drift and enforce different rules.

Issues [#32](https://github.com/ahstn/oceans-llm/issues/32) and [#33](https://github.com/ahstn/oceans-llm/issues/33) close that gap by turning identity lifecycle and team-member mutation into an explicit, backend-enforced slice instead of a collection of ad hoc UI actions.

## Decision

We treat admin identity lifecycle and team-member mutation as a single architectural concern with one source of truth in the gateway and store layers.

The key decisions are:

### 1. Model user status as a typed domain concept

User status is no longer an unstructured string carried through the system. The canonical lifecycle states now live in [../../crates/gateway-core/src/domain.rs](../../crates/gateway-core/src/domain.rs) as `UserStatus`.

Why:

- lifecycle transitions are meaningful domain rules, not display strings,
- the same states are used by HTTP handlers, store logic, runtime checks, and tests,
- typed status handling reduces drift between libsql, PostgreSQL, and the admin UI.

### 2. Centralize lifecycle policy in one backend module

The allowed transitions and guardrails live in [../../crates/gateway/src/http/identity_lifecycle.rs](../../crates/gateway/src/http/identity_lifecycle.rs).

That module owns rules such as:

- bootstrap admin is not managed through normal lifecycle flows,
- `owner` memberships cannot be created, removed, or transferred in this slice,
- auth mode can only change while a user is still `invited`,
- self-demotion and self-deactivation are blocked,
- the last active platform admin cannot be demoted or deactivated,
- reset onboarding is limited to `invited` and `disabled` users.

Why:

- these constraints are security-sensitive and must not depend on UI behavior,
- keeping them in one place makes the rules easier to audit and extend,
- route handlers can stay thin and delegate policy decisions instead of re-encoding them.

### 3. Use explicit action endpoints for destructive lifecycle operations

We kept non-destructive edits on `PATCH /api/v1/admin/identity/users/{user_id}`, but destructive or stateful transitions use explicit action routes in [../../crates/gateway/src/http/mod.rs](../../crates/gateway/src/http/mod.rs) and [../../crates/gateway/src/http/identity.rs](../../crates/gateway/src/http/identity.rs):

- `POST /api/v1/admin/identity/users/{user_id}/deactivate`
- `POST /api/v1/admin/identity/users/{user_id}/reactivate`
- `POST /api/v1/admin/identity/users/{user_id}/reset-onboarding`
- `DELETE /api/v1/admin/identity/teams/{team_id}/members/{user_id}`
- `POST /api/v1/admin/identity/teams/{team_id}/members/{user_id}/transfer`

Why:

- named actions communicate that these operations are not generic field updates,
- the API surface matches the operational mental model used by admins,
- separate actions reduce ambiguity around partial failure and confirmation UX.

### 4. Treat deactivation as an access-control event, not a cosmetic status change

Deactivation revokes active sessions and makes runtime access ineffective immediately. The relevant enforcement paths are in:

- [../../crates/gateway/src/http/identity.rs](../../crates/gateway/src/http/identity.rs)
- [../../crates/gateway-service/src/model_access.rs](../../crates/gateway-service/src/model_access.rs)

Why:

- a disabled account that can still act through a stale session is not actually disabled,
- lifecycle status must affect both admin control-plane access and runtime data-plane access,
- user-owned API keys must not remain valid when the owning user is no longer active.

### 5. Keep transfer semantics intentionally narrow

Team transfer changes the user’s current membership and future membership-derived access only. It does not rewrite historical request logs, spend, budgets, or API-key ownership.

Why:

- those records are historical or independently owned data,
- automatic reattribution would be surprising and difficult to audit,
- narrow transfer semantics make the operation safe to explain and safe to implement.

### 6. Keep `owner` handling deferred rather than pretending it is solved

The system may contain `owner` memberships, but this slice does not let admins create, remove, or transfer them through the new workflows.

Why:

- `owner` implies stronger responsibility boundaries than `admin` and `member`,
- the codebase does not yet have a complete ownership migration story,
- blocking the workflow is safer than allowing incomplete semantics to leak into production.

## Implementation

This ADR is intentionally about both why and how. The implementation is spread across a few clear layers.

### Domain and policy

- [../../crates/gateway-core/src/domain.rs](../../crates/gateway-core/src/domain.rs)
  - introduces `UserStatus`
  - moves status parsing and formatting toward typed handling
- [../../crates/gateway/src/http/identity_lifecycle.rs](../../crates/gateway/src/http/identity_lifecycle.rs)
  - centralizes lifecycle transition validation
  - defines the control-plane invariants for this slice

This combination turns identity lifecycle into a domain rule set instead of a bag of route-local string checks.

### Gateway HTTP layer

- [../../crates/gateway/src/http/identity.rs](../../crates/gateway/src/http/identity.rs)
  - adds request types for user update and team transfer
  - implements lifecycle and team-member mutation handlers
  - reuses onboarding response shapes for reset-onboarding
  - expands team views to include real member rosters for the admin UI
- [../../crates/gateway/src/http/mod.rs](../../crates/gateway/src/http/mod.rs)
  - wires the new admin routes into the gateway
- [../../crates/gateway/src/http/admin_auth.rs](../../crates/gateway/src/http/admin_auth.rs)
  - now relies on typed `UserStatus` checks

This keeps the gateway as the single backend boundary for identity mutation. The admin UI does not apply business rules on its own and the store does not expose policy-free mutation semantics directly to clients.

### Store layer and transactional behavior

- [../../crates/gateway-store/src/store.rs](../../crates/gateway-store/src/store.rs)
  - extends the store contract with explicit lifecycle and membership mutation helpers
- [../../crates/gateway-store/src/libsql_store/identity.rs](../../crates/gateway-store/src/libsql_store/identity.rs)
- [../../crates/gateway-store/src/postgres_store/identity.rs](../../crates/gateway-store/src/postgres_store/identity.rs)
  - implement atomic user updates
  - implement membership removal and transfer
  - revoke sessions
  - clear password and OIDC auth records
  - count active platform admins

The important implementation choice here is transactional explicitness. Transfer is implemented as one backend operation under the existing one-team-per-user constraint rather than as a remove-then-add sequence exposed to callers. That prevents partial state from leaking across the unique membership boundary.

We deliberately did not introduce a schema migration for this slice. The schema was already capable of representing the needed state; what was missing was the repository surface area and the transaction semantics to manipulate that state safely.

### Runtime enforcement

- [../../crates/gateway-service/src/model_access.rs](../../crates/gateway-service/src/model_access.rs)
  - blocks disabled users from continuing to use user-owned API keys
- [../../crates/gateway/src/http/identity.rs](../../crates/gateway/src/http/identity.rs)
  - invalidates disabled users during session resolution
  - blocks password-change flows for non-active users

This matters because admin lifecycle operations are only credible if they affect the real runtime boundary.

### Admin UI and control-plane UX

- [../../crates/admin-ui/web/src/routes/identity/users.tsx](../../crates/admin-ui/web/src/routes/identity/users.tsx)
  - adds edit and lifecycle actions for users
- [../../crates/admin-ui/web/src/routes/identity/teams.tsx](../../crates/admin-ui/web/src/routes/identity/teams.tsx)
  - adds member roster, removal, and transfer flows
- [../../crates/admin-ui/web/src/server/admin-data.server.ts](../../crates/admin-ui/web/src/server/admin-data.server.ts)
- [../../crates/admin-ui/web/src/server/admin-data.functions.ts](../../crates/admin-ui/web/src/server/admin-data.functions.ts)
- [../../crates/admin-ui/web/src/types/api.ts](../../crates/admin-ui/web/src/types/api.ts)
  - align the admin UI contract with the gateway endpoints and request shapes

The UI remains a thin control plane over same-origin server functions and route invalidation. That preserves the project’s preferred pattern: the backend is authoritative, while the UI focuses on operator clarity and confirmation copy.

### Tests and documentation

- [../../crates/gateway-store/src/lib.rs](../../crates/gateway-store/src/lib.rs)
  - adds store-level coverage for transfer, removal, owner blocking, session revocation, and admin counting
- [../../crates/gateway/src/main.rs](../../crates/gateway/src/main.rs)
  - adds end-to-end gateway tests for lifecycle transitions and member workflows
- [../../crates/admin-ui/web/src/server/admin-data.server.test.ts](../../crates/admin-ui/web/src/server/admin-data.server.test.ts)
- [../../crates/admin-ui/web/src/test/routes/users-route.test.tsx](../../crates/admin-ui/web/src/test/routes/users-route.test.tsx)
- [../../crates/admin-ui/web/src/test/routes/teams-route.test.tsx](../../crates/admin-ui/web/src/test/routes/teams-route.test.tsx)
  - cover the server contract and the new admin flows

Canonical docs were updated alongside the code:

- [../identity-and-access.md](../access/identity-and-access.md)
- [../admin-control-plane.md](../access/admin-control-plane.md)
- [../e2e-contract-tests.md](../reference/e2e-contract-tests.md)

## Consequences

Positive:

- admins can now manage the real lifecycle of an account without recreating users,
- team membership changes are explicit backend operations instead of manual repair work,
- the last-admin and bootstrap-admin safeguards now live in code, not just tribal knowledge,
- disabled users lose access in practice, not just in the admin table,
- the same rules apply across libsql, PostgreSQL, gateway handlers, and the UI.

Tradeoffs:

- `owner` lifecycle remains intentionally unresolved,
- auth-mode switching is conservative and restricted to the invited state,
- transfer solves operational reorganization, not historical data reattribution,
- the gateway now owns more policy logic, which is appropriate but increases the need for disciplined test coverage.

## Alternatives Considered

### 1. Keep status as strings and enforce transitions in each handler

Rejected because it would repeat security-sensitive rules across multiple call sites and make store/runtime drift more likely.

### 2. Allow the UI to enforce most lifecycle restrictions

Rejected because UI-only enforcement would be bypassable and would not protect direct API callers or future admin surfaces.

### 3. Implement transfer as remove-then-add

Rejected because the existing one-team-per-user uniqueness rule makes that sequence too easy to partially apply or mis-handle under failure.

### 4. Treat transfer as ownership and accounting migration

Rejected because the system is not yet designed to safely or predictably rewrite historical data that way.

## Follow-Up Work

- Define a dedicated ownership lifecycle before exposing `owner` mutation in the admin UI.
- Decide whether other services or SDKs should consume a shared lifecycle contract rather than relying on HTTP semantics alone.
- Expand end-to-end coverage as team-owned budgets, API keys, and other ownership-bound resources become more editable.
- Revisit whether some lifecycle policy should move lower than the HTTP layer if additional non-HTTP mutation paths are introduced in the future.

## Attribution

This ADR was prepared through collaborative human + AI implementation and documentation work.
