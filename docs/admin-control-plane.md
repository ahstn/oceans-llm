# Admin Control Plane

`Owns`: the current admin UI capability map, live versus preview-backed surfaces, and operator expectations for the control plane.
`Depends on`: [identity-and-access.md](identity-and-access.md), [budgets-and-spending.md](budgets-and-spending.md), [observability-and-request-logs.md](observability-and-request-logs.md)
`See also`: [e2e-contract-tests.md](e2e-contract-tests.md), [../crates/gateway/src/http/admin_contract.rs](../crates/gateway/src/http/admin_contract.rs), [../crates/gateway/openapi/admin-api.json](../crates/gateway/openapi/admin-api.json), [../crates/admin-ui/web/src/generated/admin-api.ts](../crates/admin-ui/web/src/generated/admin-api.ts)

This document describes what operators can actually do in the admin UI today.

## Same-Origin Control Plane

The control plane is served through the gateway at `/admin`.

Normal runtime model:

- gateway handles auth, admin APIs, and reverse proxying
- the Bun SSR app calls back into the gateway using a typed same-origin client with forwarded-origin resolution

Live admin and auth surfaces now use a generated contract pipeline:

- Rust transport DTOs and handler annotations live in [../crates/gateway/src/http/admin_contract.rs](../crates/gateway/src/http/admin_contract.rs)
- the checked-in OpenAPI artifact lives at [../crates/gateway/openapi/admin-api.json](../crates/gateway/openapi/admin-api.json)
- the admin UI consumes generated types from [../crates/admin-ui/web/src/generated/admin-api.ts](../crates/admin-ui/web/src/generated/admin-api.ts)

For local direct UI dev on `:3001`, the server-side gateway client falls back to `:8080` unless `ADMIN_GATEWAY_ORIGIN` is explicitly set.

## Live Gateway-Backed Surfaces

These areas are backed by real gateway APIs today:

- sign-in, session lookup, and password rotation
- identity users and lifecycle management
- identity teams and member transfer/removal workflows
- password invites and onboarding links
- OIDC pre-provisioning flows
- spend usage reporting
- spend budget management for users and teams
- request-log list and detail inspection

Those live surfaces are expected to track the generated gateway contract directly. We do not keep a separate hand-maintained frontend transport model for them.

## Preview-Backed Surfaces

These pages are still powered by local preview data in the admin UI:

- API Keys
- Models

That is not just an implementation detail. It affects both operator expectations and test scope.

Tracked follow-ups:

- [issue #26](https://github.com/ahstn/oceans-llm/issues/26): replace preview API-key data with live management
- [issue #27](https://github.com/ahstn/oceans-llm/issues/27): replace preview model inventory with live routing and provider state

## Operator-Visible Maturity Cues

The admin UI currently teaches this maturity split in live copy and tests:

- identity and spend surfaces are live gateway-backed contracts
- API keys and models are intentionally preview-backed in this slice

That message is part of the operator contract and should be treated as owned by this page rather than only by UI fixture code or E2E assertions.

## Identity Workflows Available Today

Operators can currently:

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

Current scope limits:

- no admin logout/session-management flow yet
- owner memberships are visible but blocked from removal/transfer in this slice
- auth-mode switching is limited to invited users
- OIDC remains development-style, not hardened

## Auth And Session UX Limits

Current session state is mostly implicit:

- browser cookie state carries the admin session
- `/api/v1/auth/session` is the main machine-readable session lookup
- there is not yet a dedicated logout/session-management shell in the control plane

Related follow-ups:

- [issue #34](https://github.com/ahstn/oceans-llm/issues/34)
- [issue #33](https://github.com/ahstn/oceans-llm/issues/33)
- [issue #46](https://github.com/ahstn/oceans-llm/issues/46)

## Spend and Observability Workflows Available Today

Operators can currently:

- inspect 7- and 30-day spend windows
- filter spend by owner kind
- manage user and team budgets
- inspect request-log summaries
- filter request logs by caller service, component, environment, and one bespoke tag match
- inspect sanitized request-log payload detail

Current scope limits:

- spend reporting does not yet include provider breakdown
- request-log detail missing rows now return `404 not_found`
- request-log filtering and ergonomics still have follow-up work

Related follow-ups:

- [issue #45](https://github.com/ahstn/oceans-llm/issues/45)
- [issue #20](https://github.com/ahstn/oceans-llm/issues/20)
- [issue #50](https://github.com/ahstn/oceans-llm/issues/50)

## Relationship to Testing

The E2E harness treats only live gateway-backed surfaces as contract flows.

- live surfaces should gain targeted cross-layer coverage as they harden
- preview-backed pages can appear in landing assertions, but not as business-flow coverage
- user lifecycle and team member workflows now belong in the live contract suite

See [e2e-contract-tests.md](e2e-contract-tests.md).
