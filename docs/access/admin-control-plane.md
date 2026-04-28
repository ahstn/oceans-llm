# Admin Control Plane

`See also`: [Identity and Access](identity-and-access.md), [Budgets and Spending](../operations/budgets-and-spending.md), [Observability and Request Logs](../operations/observability-and-request-logs.md), [Admin API Contract Workflow](../reference/admin-api-contract-workflow.md), [End-to-End Contract Tests](../reference/e2e-contract-tests.md), [OIDC and SSO Status](oidc-and-sso-status.md)

This page describes what admins can actually do in the admin UI today.

## Same-Origin Control Plane

The control plane is served through the gateway at `/admin`.

Normal runtime model:

- the gateway handles auth, admin APIs, and reverse proxying
- the SSR app calls back into the gateway through the same-origin client boundary

For the generated contract and artifact workflow, use [admin-api-contract-workflow.md](../reference/admin-api-contract-workflow.md).

## Live Gateway-Backed Surfaces

These areas are backed by real gateway APIs today:

- sign-in, session lookup, current-session logout, and password rotation
- API key inventory, creation, and revocation
- identity users and lifecycle management
- identity teams and member transfer or removal workflows
- password invite and onboarding links
- OIDC pre-provisioning flows
- spend usage reporting
- spend budget management for users and teams
- request-log list and detail inspection

## Live But Still Maturing Surfaces

These pages now read from gateway APIs, but still have capability-detail follow-up work:

- Models

That split matters for admin expectations and test scope.

## Admin-Visible Maturity Cues

The current product contract is mixed on purpose:

- identity, spend, API keys, request logs, leaderboard, and Models are live gateway-backed surfaces
- Models still needs richer runtime capability visibility, including Responses support

Tracked follow-up:

- [issue #27](https://github.com/ahstn/oceans-llm/issues/27)
- [issue #96](https://github.com/ahstn/oceans-llm/issues/96)

## API-Key Workflows Available Today

Admins can:

- list API keys with owner summary and grant list
- create a new key for an explicit user or team owner
- grant access to an explicit set of gateway models at creation time
- copy the raw key once from the create response
- replace model grants for an active key
- revoke a key so runtime auth rejects it immediately

Current limits:

- no rename flow
- no owner transfer flow
- no secret recovery flow
- no restore-from-revoked flow
- revoked keys are read-only
- model choice is limited to the live gateway model catalog

## Identity Workflows Available Today

Admins can:

- sign in as the bootstrap or existing platform admin
- rotate the bootstrap password when required
- create users
- edit user role and membership fields
- deactivate, reactivate, and reset onboarding for users
- create teams
- add existing users to teams
- invite new users directly into teams
- pre-provision OIDC users against enabled providers
- remove team members
- transfer team members between teams with an explicit destination role
- sign out of the current browser session

Current limits:

- owner memberships stay blocked from removal or transfer in this slice
- auth-mode switching is limited to invited users
- OIDC is still development-style

## Admin Auth and Session Behavior

Only `platform_admin` accounts can enter the admin control plane. Ordinary `user` accounts are data-plane identities unless their global role is changed.

Current session behavior is cookie-backed and admin-visible:

- browser cookie state carries the admin session
- `/api/v1/auth/session` is the machine-readable session lookup
- `/api/v1/auth/logout` revokes the current cookie-backed session and clears the browser cookie
- expired or missing session state sends the admin back through the auth flow
- bootstrap admin and regular admin accounts share the same session mechanics after sign-in

What is still missing:

- broader session-management UI

## Spend and Observability Workflows Available Today

Admins can:

- inspect 7-day and 30-day spend windows
- filter spend by owner kind
- manage user and team budgets
- inspect the 7-day or 31-day usage leaderboard
- inspect request-log summaries
- filter request logs by caller service, component, environment, and one bespoke tag match
- inspect sanitized request-log payload detail
- see per-row payload capture mode, byte limits, stream event limit, policy version, and truncation state
- see per-row MCP/tool cardinality counts for request logs
- compare leaderboard users with average tool exposure and invocation counts

Request-log payload policy is read-only in the admin UI. Admins configure it through `gateway.yaml`; see [observability-and-request-logs.md](../operations/observability-and-request-logs.md).

Current limits:

- spend reporting still lacks provider breakdown
- admin mutation audit logs are still tracked separately in [issue #99](https://github.com/ahstn/oceans-llm/issues/99)
- request-log detail missing rows return `404 not_found`
- request-log filtering ergonomics still have follow-up work

## Non-Human Callers Today

The gateway does not yet have first-class gateway service accounts.

Current choices are:

- user-owned API keys
- team-owned API keys
- config-seeded keys owned by the reserved `system-legacy` team

Do not confuse this with provider credential auth such as Vertex `auth.mode: service_account`. Gateway service-account semantics are tracked in [issue #107](https://github.com/ahstn/oceans-llm/issues/107).

## Current Gaps

- OIDC still development-style:
  - [oidc-and-sso-status.md](oidc-and-sso-status.md)
- Models capability detail still maturing:
  - [issue #27](https://github.com/ahstn/oceans-llm/issues/27)
  - [issue #96](https://github.com/ahstn/oceans-llm/issues/96)

## Relationship to Testing

The E2E harness treats only live gateway-backed surfaces as contract flows.

- live surfaces should gain targeted cross-layer coverage as they harden
- maturing live pages can appear in smoke coverage before every workflow becomes business-flow coverage

Use [e2e-contract-tests.md](../reference/e2e-contract-tests.md) for the test boundary.
