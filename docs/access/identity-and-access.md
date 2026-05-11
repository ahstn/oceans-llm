# Identity and Access

`See also`: [Data Relationships](../reference/data-relationships.md), [Runtime Bootstrap and Access](../setup/runtime-bootstrap-and-access.md), [Service Accounts](service-accounts.md), [OIDC and SSO Status](oidc-and-sso-status.md), [Admin Control Plane](admin-control-plane.md), [Budgets and Spending](../operations/budgets-and-spending.md), [MCP Invocations](../operations/observability/mcp-invocations.md), [ADR: Team Service Accounts for Non-Human Gateway Access](../adr/2026-05-10-team-service-accounts.md), [ADR: Admin Identity Lifecycle and Team Member Workflow Hardening](../adr/2026-03-26-admin-identity-lifecycle-and-team-member-workflows.md)

This page describes the live identity model across the gateway and admin control plane.

## Source of Truth

- identity APIs:
  - [../crates/gateway/src/http/identity.rs](../../crates/gateway/src/http/identity.rs)
- lifecycle policy:
  - [../crates/gateway/src/http/identity_lifecycle.rs](../../crates/gateway/src/http/identity_lifecycle.rs)
- access evaluation:
  - [../crates/gateway-service/src/model_access.rs](../../crates/gateway-service/src/model_access.rs)

## Ownership Model

The product uses first-class users, teams, service accounts, and API-key credentials.

- Users and service accounts are gateway principals.
- API keys are credentials attached to a user or service account.
- Teams are durable ownership boundaries for team budgets and team-owned service accounts.
- Service accounts are first-class team-owned non-human gateway principals.
- Direct team-owned runtime API keys are not part of the product contract.
- One user belongs to at most one team in this slice.
- Users can exist without a team.
- There is no reserved `system-legacy` team or system-legacy runtime-key compatibility.

Gateway service-account-style callers are modeled with API keys today, not a distinct owner kind.

- Use a team-owned API key for shared automation or service workloads.
- Use a user-owned API key only for traffic that should spend against one user.
- Use config-seeded keys for bootstrap or deployment-managed callers, knowing they are owned by `system-legacy`.
- Keep provider service-account credentials out of this identity model; they belong to provider config.

## User Lifecycle

User status is typed, not free-form text.

- `invited`
- `active`
- `disabled`

Important rules:

- auth-mode changes are only allowed while the user is still `invited`
- deactivation revokes runtime access
- reactivation only restores access when the current auth proof still exists
- reset-onboarding returns the user to `invited`
- the last active platform admin cannot be deactivated or demoted
- the bootstrap admin stays out of normal user-management views

## Admin Session Lifecycle

Admin sessions are durable server-side records referenced by the `ogw_session` browser cookie.

- normal sign-out revokes only the current cookie-backed session
- logout is idempotent and clears the browser cookie even when the session is already gone
- user lifecycle actions such as deactivation can revoke every active session for that user
- expired, revoked, missing, or disabled-user sessions resolve as unauthenticated and return the admin to sign-in

## Bootstrap Admin

Bootstrap admin is the first control-plane access path, not a normal user-management path.

- local config keeps it enabled without forced password rotation
- production-shaped local config keeps it enabled with forced password rotation
- the active config and startup toggles decide whether it is created on boot

For the startup and first-access path, use [runtime-bootstrap-and-access.md](../setup/runtime-bootstrap-and-access.md).

## Onboarding Model

Current onboarding is admin-mediated.

- admins create users or invite them into teams
- the control plane generates password invite links or OIDC sign-in links
- the admin shares that link out of band

There is no self-service discovery flow in this slice.

## Team Lifecycle

Current team-management rules:

- teams can be created before users exist
- teams can be created with zero admins
- the admin UI can add existing teamless users or invite new users directly into a team
- team owners and admins can manage service accounts for their own team
- platform admins can manage service accounts across teams
- non-owner memberships can be transferred between teams
- `owner` memberships are visible but blocked from casual lifecycle edits

## Team Transfer Rule

Team transfer is easy to overread. The rule is narrow on purpose.

Transfer changes:

- the user’s current membership
- future membership-derived access

Transfer does not change:

- historical request logs
- historical spend rows
- existing budgets
- API-key ownership
- service-account ownership

That boundary is a policy rule, not a UI shortcut.

## OIDC Boundary

OIDC exists in the product, but it is still development-style in this slice.

- pre-provisioned OIDC users are supported
- config-seeded OIDC users can be pre-provisioned by provider key
- OIDC onboarding links exist
- hardened production-grade SSO is still a follow-up

Use [oidc-and-sso-status.md](oidc-and-sso-status.md) for the practical boundary and future direction.

## Model Access Overlays

Effective model access is layered:

1. API key grants for the authenticated user or service account credential
2. team allowlist when the team is `restricted`
3. user allowlist when the user is `restricted`

This keeps API-key grants as the baseline contract while allowing narrower restrictions above them.

For service accounts, the team allowlist applies through the owning team. User allowlists do not apply because service accounts are not users.

## Request Logging Preference

Request logging policy is partly owned by identity.

- user-owned requests honor `users.request_logging_enabled`
- service-account requests always persist request-log summary rows
- the admin identity view exposes the current user preference read-only

MCP invocation logging follows the same ownership vocabulary for audit context. Invocation rows should preserve the API key, user, and team ids available at execution time, but they do not rewrite historical ownership when a user changes teams or an API key is revoked later.

## Declarative Identity Seed

Config-backed identity is now part of the startup seed path.

- `teams` are reconciled by `team_key`
- `users` are reconciled by normalized email
- listed users can reconcile team membership and active budgets
- new config-seeded users start as `invited`
- config seeding does not emit invite URLs; admins generate onboarding links from the admin UI when needed

Config seeding no longer creates legacy system-owned runtime API keys. Non-human team access is managed through service accounts.

## Service Accounts

Service accounts are the non-human gateway identity model.

- each service account belongs to exactly one team
- service accounts cannot sign in to `/admin`
- service-account credentials can call `/v1/*`
- deletion is deactivation
- service-account budget alerts go to active owning-team owners and admins

Team-scoped management rules live in [service-accounts.md](service-accounts.md).

## Current Gaps

- Self-hosted test-IdP guidance is still pending:
  - [issue #46](https://github.com/ahstn/oceans-llm/issues/46)
- Hardened declarative SSO-backed identity matching is still pending:
  - [issue #65](https://github.com/ahstn/oceans-llm/issues/65)
- Standards-complete OIDC tracking needs a reopened or successor issue because [issue #29](https://github.com/ahstn/oceans-llm/issues/29) is closed while the current docs still describe development-style OIDC behavior.

## Where Identity Appears Operationally

- admin workflows:
  - [admin-control-plane.md](admin-control-plane.md)
- startup and first access:
  - [runtime-bootstrap-and-access.md](../setup/runtime-bootstrap-and-access.md)
- spend ownership effects:
  - [budgets-and-spending.md](../operations/budgets-and-spending.md)
- request resolution effects:
  - [model-routing-and-api-behavior.md](../configuration/model-routing-and-api-behavior.md)
- non-human gateway access:
  - [service-accounts.md](service-accounts.md)
