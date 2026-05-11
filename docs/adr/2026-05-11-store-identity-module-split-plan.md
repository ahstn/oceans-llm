# ADR: Store Identity Module Split Plan

## Status

Proposed

## Context

The libsql and Postgres identity store modules now own users, teams, memberships, OIDC links, sessions, and service accounts. The libsql implementation has crossed the repository's file-size review trigger, and the Postgres implementation has the same cohesion pressure even though it is currently smaller.

## Decision

Keep the service-account changes in the existing identity store modules for this feature branch, then split the identity store by repository concern before adding another identity surface.

The intended split is:

1. `users`: user profiles, password/OIDC/OAuth records, invitations, and sessions.
2. `teams`: teams, memberships, and team model allowlists.
3. `service_accounts`: service-account lifecycle and service-account model allowlists.

Each backend should keep a thin `identity` module that re-exports the concern modules and preserves the existing public store API. Tests should move with the behavior they cover rather than staying in a single identity test block.

## Rationale

This avoids mixing a structural refactor into the service-account rollout while still setting a concrete boundary for future work. Service accounts are a first-class identity concern, but their lifecycle and allowlist behavior can be maintained independently from human-user and team-membership storage.

## Follow-Up

- Split `crates/gateway-store/src/libsql_store/identity.rs` into concern modules.
- Apply the same module shape to `crates/gateway-store/src/postgres_store/identity.rs`.
- Move identity-store tests into matching concern-scoped modules.
