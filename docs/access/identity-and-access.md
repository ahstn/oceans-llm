# Identity and Access

`See also`: [Data Relationships](../reference/data-relationships.md), [Runtime Bootstrap and Access](../setup/runtime-bootstrap-and-access.md), [OIDC and SSO Status](oidc-and-sso-status.md), [Admin Control Plane](admin-control-plane.md), [Budgets and Spending](../operations/budgets-and-spending.md), [ADR: Admin Identity Lifecycle and Team Member Workflow Hardening](../adr/2026-03-26-admin-identity-lifecycle-and-team-member-workflows.md)

This page describes the live identity model across the gateway and admin control plane.

## Source of Truth

- identity APIs:
  - [../crates/gateway/src/http/identity.rs](../../crates/gateway/src/http/identity.rs)
- lifecycle policy:
  - [../crates/gateway/src/http/identity_lifecycle.rs](../../crates/gateway/src/http/identity_lifecycle.rs)
- access evaluation:
  - [../crates/gateway-service/src/model_access.rs](../../crates/gateway-service/src/model_access.rs)

## Ownership Model

The product uses first-class users, teams, and API-key ownership.

- API keys are either user-owned or team-owned.
- Teams are durable ownership boundaries for team budgets and future team-owned resources.
- One user belongs to at most one team in this slice.
- Users can exist without a team.
- Legacy keys are preserved through the reserved `system-legacy` team.

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

## Bootstrap Admin

Bootstrap admin is the first control-plane access path, not a normal user-management path.

- local config keeps it enabled without forced password rotation
- production-shaped local config keeps it enabled with forced password rotation
- the active config and startup toggles decide whether it is created on boot

For the startup and first-access path, use [runtime-bootstrap-and-access.md](../setup/runtime-bootstrap-and-access.md).

## Onboarding Model

Current onboarding is operator-mediated.

- admins create users or invite them into teams
- the control plane generates password invite links or OIDC sign-in links
- the admin shares that link out of band

There is no self-service discovery flow in this slice.

## Team Lifecycle

Current team-management rules:

- teams can be created before users exist
- teams can be created with zero admins
- the admin UI can add existing teamless users or invite new users directly into a team
- cross-team reassignment is rejected in this slice
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

That boundary is a policy rule, not a UI shortcut.

## OIDC Boundary

OIDC exists in the product, but it is still development-style in this slice.

- pre-provisioned OIDC users are supported
- OIDC onboarding links exist
- hardened production-grade SSO is still a follow-up

Use [oidc-and-sso-status.md](oidc-and-sso-status.md) for the practical boundary and future direction.

## Model Access Overlays

Effective model access is layered:

1. API key grants
2. team allowlist when the team is `restricted`
3. user allowlist when the user is `restricted`

This keeps API-key grants as the baseline contract while allowing narrower restrictions above them.

## Request Logging Preference

Request logging policy is partly owned by identity.

- user-owned requests honor `users.request_logging_enabled`
- team-owned requests always persist request logs

## Current Gaps

- Hardened OIDC flow is still pending:
  - [issue #29](https://github.com/ahstn/oceans-llm/issues/29)
- Self-hosted test-IdP guidance is still pending:
  - [issue #46](https://github.com/ahstn/oceans-llm/issues/46)
- Declarative config-driven identity is still pending:
  - [issue #64](https://github.com/ahstn/oceans-llm/issues/64)
  - [issue #65](https://github.com/ahstn/oceans-llm/issues/65)

## Where Identity Appears Operationally

- admin workflows:
  - [admin-control-plane.md](admin-control-plane.md)
- startup and first access:
  - [runtime-bootstrap-and-access.md](../setup/runtime-bootstrap-and-access.md)
- spend ownership effects:
  - [budgets-and-spending.md](../operations/budgets-and-spending.md)
- request resolution effects:
  - [model-routing-and-api-behavior.md](../configuration/model-routing-and-api-behavior.md)
