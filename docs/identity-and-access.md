# Identity and Access

`Owns`: bootstrap admin behavior, users, teams, onboarding, ownership model, request-logging preference, and access overlays.
`Depends on`: [data-relationships.md](data-relationships.md)
`See also`: [admin-control-plane.md](admin-control-plane.md), [budgets-and-spending.md](budgets-and-spending.md), [adr/2026-03-05-identity-foundation.md](adr/2026-03-05-identity-foundation.md), [adr/2026-03-08-admin-team-management-flow.md](adr/2026-03-08-admin-team-management-flow.md)

This document describes the live identity and access model across the gateway and admin control plane.

## Source of Truth

- Identity APIs: [../crates/gateway/src/http/identity.rs](../crates/gateway/src/http/identity.rs)
- Access evaluation: [../crates/gateway-service/src/model_access.rs](../crates/gateway-service/src/model_access.rs)
- Bootstrap admin creation: [../crates/gateway/src/main.rs](../crates/gateway/src/main.rs)

## Ownership Model

The product uses first-class users, teams, and API key ownership:

- API keys are either user-owned or team-owned
- teams are durable ownership boundaries for team budgets and future team-owned resources
- one user belongs to at most one team in this slice
- users can exist without a team

Legacy keys are preserved through the reserved `system-legacy` team.

## Bootstrap Admin

The gateway can ensure a bootstrap platform admin exists at startup.

Default checked-in behavior:

- local config (`gateway.yaml`): `admin@local` / `admin`, no forced password change
- production-shaped config (`gateway.prod.yaml`): `admin@local` / `admin`, forced password change on first login

Relevant controls:

- `GATEWAY_BOOTSTRAP_ADMIN`
- `gateway bootstrap-admin`
- `auth.bootstrap_admin` in the active YAML config

## Admin Auth and Onboarding

The current admin/auth surface includes:

- password login
- password rotation
- password invite validation and completion
- pre-provisioned OIDC sign-in
- authenticated admin session lookup

Password onboarding:

- invited password users receive a time-limited invitation token
- setting the password activates the user

OIDC onboarding:

- the admin UI can pre-provision an invited OIDC user against an enabled provider
- first successful callback activates the user and creates the durable provider subject link

## Onboarding Handoff Model

Admin onboarding is intentionally operator-mediated today:

- admins create users or invite them into teams
- the control plane generates password invite URLs or OIDC sign-in URLs
- the admin then shares that onboarding link out of band

That handoff model is part of the current product contract. There is no separate self-service discovery flow in this slice.

## Important Current Limitation: OIDC Is Still Development-Style

Current OIDC behavior is intentionally not a hardened standards-complete provider flow.

Today the implementation still:

- redirects `oidc_start` directly into the callback path
- accepts callback identity through query parameters
- synthesizes the provider subject as `mock:{provider_key}:{email}` when no explicit subject is provided

That is useful for local and slice-level testing, but it is not the final production design. The follow-up direction is tracked in [issue #46](https://github.com/ahstn/oceans-llm/issues/46), and the earlier hardening intent is recorded in [issue #29](https://github.com/ahstn/oceans-llm/issues/29).

## Teams

Current team-management rules:

- teams can be created before users exist
- teams can be created with zero admins
- the admin UI can add existing teamless users or invite new members directly into a team
- cross-team reassignment is rejected in this slice
- `owner` remains a backend concept and is not exposed as a general admin-UI lifecycle today

## Current Team Lifecycle Boundaries

Additional boundaries that are easy to miss from one page or one API response:

- `team_key` is server-generated and durable
- empty teams are valid and expected
- current edit flows primarily synchronize the admin subset, not a full membership lifecycle model
- removal and transfer flows remain deferred follow-up work

## Model Access Overlays

Effective model access is layered:

1. API key grants
2. team allowlist when the team is `restricted`
3. user allowlist when the user is `restricted`

This keeps API key grants as the baseline contract while allowing narrower team or user restrictions above them.

## Request Logging Preference

Request logging policy is owned partly by identity:

- user-owned requests honor `users.request_logging_enabled`
- team-owned requests always persist request logs

Request-log storage and observability behavior are documented in [observability-and-request-logs.md](observability-and-request-logs.md).

## Where Identity Appears Operationally

- [Admin Control Plane](admin-control-plane.md): what admins can manage today
- [Budgets and Spending](budgets-and-spending.md): how user and team ownership affects spend enforcement
- [Model Routing and API Behavior](model-routing-and-api-behavior.md): how model grants and overlays affect `/v1/models` and request resolution
