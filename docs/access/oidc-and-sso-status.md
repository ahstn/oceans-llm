# OIDC and SSO Status

`Owns`: the current OIDC state, the boundary of the development-style flow, the planned hardened direction, and the missing local test-IdP story.
`Depends on`: [identity-and-access.md](identity-and-access.md), [runtime-bootstrap-and-access.md](../setup/runtime-bootstrap-and-access.md)
`See also`: [configuration-reference.md](../configuration/configuration-reference.md), [deploy-and-operations.md](../setup/deploy-and-operations.md), [admin-control-plane.md](admin-control-plane.md), [adr/2026-03-05-identity-foundation.md](../adr/2026-03-05-identity-foundation.md)

This page exists because the current OIDC story is easy to overread. OIDC exists in the product, but it is not hardened production-grade SSO yet.

## Current State

The live product supports pre-provisioned OIDC users and OIDC-flavored onboarding flows in the admin control plane.

What exists today:

- OIDC providers can be configured
- admins can pre-provision invited OIDC users
- first successful callback activates the invited user
- durable user and provider-link records are stored

What does not exist yet:

- a hardened standards-complete login flow
- a finished self-hosted local test-IdP story
- declarative config-as-code support for SSO-backed users and team assignment

## Development-Style Boundary

The current flow is intentionally development-style.

Today the implementation still:

- redirects `oidc_start` into the callback path directly
- accepts callback identity through query parameters
- synthesizes a provider subject when one is not supplied

That is enough for slice-level testing and UI wiring. It is not enough to describe as finished enterprise SSO.

## Practical Operator Impact

Operators should assume these boundaries:

- OIDC is usable for controlled environments and development-style testing
- OIDC is not the hardened final story for production sign-in policy
- local testing still lacks a checked-in IdP recommendation and workflow
- deploy docs should not imply that SSO-first bootstrap is solved

## Planned Direction

The current forward path is visible in repo history.

- Harden the OIDC flow itself:
  - [issue #29](https://github.com/ahstn/oceans-llm/issues/29)
- Pick and document a self-hosted test IdP story:
  - [issue #46](https://github.com/ahstn/oceans-llm/issues/46)
- Extend declarative config once hardened identity matching exists:
  - [issue #65](https://github.com/ahstn/oceans-llm/issues/65)

That sequence matters. Declarative SSO-backed users depend on the hardened identity contract, not the other way around.

## Local Test-IdP Gap

The repo does not yet ship a recommended local IdP stack for realistic OIDC testing.

That means:

- local validation remains ad hoc
- the current OIDC path is easier to demo than to validate against real provider quirks
- manual test guidance should stay conservative until the IdP choice is made

## Current Gaps

- Hardened OIDC flow: [issue #29](https://github.com/ahstn/oceans-llm/issues/29)
- Self-hosted test-IdP research: [issue #46](https://github.com/ahstn/oceans-llm/issues/46)
- Declarative SSO-backed identity config: [issue #65](https://github.com/ahstn/oceans-llm/issues/65)

## What This Page Does Not Own

- user lifecycle and team rules: [identity-and-access.md](identity-and-access.md)
- config field syntax for providers: [configuration-reference.md](../configuration/configuration-reference.md)
- admin UI capability map: [admin-control-plane.md](admin-control-plane.md)
- deploy topology and first-access behavior: [deploy-and-operations.md](../setup/deploy-and-operations.md), [runtime-bootstrap-and-access.md](../setup/runtime-bootstrap-and-access.md)
