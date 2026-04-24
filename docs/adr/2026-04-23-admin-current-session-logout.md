# ADR: Admin Current-Session Logout and Session Lifecycle

- Date: 2026-04-23
- Status: Accepted

## Implemented By

- GitHub issue: [#34 Admin Logout And Session Lifecycle](https://github.com/ahstn/oceans-llm/issues/34)
- Pull request: [#104 feat(admin): add current-session logout](https://github.com/ahstn/oceans-llm/pull/104)
- Backend HTTP:
  - [../../crates/gateway/src/http/identity.rs](../../crates/gateway/src/http/identity.rs)
  - [../../crates/gateway/src/http/mod.rs](../../crates/gateway/src/http/mod.rs)
  - [../../crates/gateway/src/http/admin_contract.rs](../../crates/gateway/src/http/admin_contract.rs)
- Store boundary:
  - [../../crates/gateway-store/src/store.rs](../../crates/gateway-store/src/store.rs)
  - [../../crates/gateway-store/src/libsql_store/identity.rs](../../crates/gateway-store/src/libsql_store/identity.rs)
  - [../../crates/gateway-store/src/postgres_store/identity.rs](../../crates/gateway-store/src/postgres_store/identity.rs)
- Admin UI:
  - [../../crates/admin-ui/web/src/routes/-auth-routing.ts](../../crates/admin-ui/web/src/routes/-auth-routing.ts)
  - [../../crates/admin-ui/web/src/routes/__root.tsx](../../crates/admin-ui/web/src/routes/__root.tsx)
  - [../../crates/admin-ui/web/src/routes/-admin-guard.ts](../../crates/admin-ui/web/src/routes/-admin-guard.ts)
  - [../../crates/admin-ui/web/src/components/layout/app-shell.tsx](../../crates/admin-ui/web/src/components/layout/app-shell.tsx)
  - [../../crates/admin-ui/web/src/components/app-sidebar.tsx](../../crates/admin-ui/web/src/components/app-sidebar.tsx)
- Docs:
  - [../access/admin-control-plane.md](../access/admin-control-plane.md)
  - [../access/identity-and-access.md](../access/identity-and-access.md)
  - [../reference/e2e-contract-tests.md](../reference/e2e-contract-tests.md)

## Context

Before issue #34, the admin control plane had durable server-side sessions but no operator-visible sign-out path. Operators could sign in, rotate a required bootstrap password, and access protected admin routes, but leaving the browser session required waiting for expiry or clearing browser state manually.

That gap was small in code but important in meaning. Session lifecycle is part of the trust boundary for the admin control plane. If logout is vague, future contributors can reasonably copy the wrong pattern: revoke every session because it is simpler, clear only the browser cookie because it is visible, or add a client-only redirect that leaves durable server state active.

At the same time, the login and route-guard code had several adjacent auth-hardening concerns:

- password-login failures could reveal whether an account existed, was inactive, was not password-backed, or lacked platform-admin privileges;
- cookie issuance did not encode the effective HTTPS scheme in the session cookie attributes;
- root and child admin guards duplicated redirect and platform-admin checks;
- the shell still displayed stale preview-era copy instead of account actions.

We were still pre-v1, so the right answer was not compatibility layering or fallback routes. The right answer was a small, explicit session lifecycle contract and removal of duplicated or stale patterns while the surface area is still tractable.

## Decision

### 1. Logout revokes the current browser session only

We added `POST /api/v1/auth/logout` as the only admin logout endpoint.

The endpoint resolves the current `ogw_session` cookie, verifies the stored session token hash when a row exists, revokes that one session when it is valid and active, and always emits a clearing cookie.

Why:

- normal sign-out should not surprise operators by invalidating other browsers or devices;
- server-side session state must be revoked, not just hidden by deleting the browser cookie;
- the durable session row remains the source of truth for whether a cookie is usable.

The endpoint is intentionally idempotent. Missing, malformed, expired, already revoked, and hash-mismatched cookies all return success and clear the cookie. Hash-mismatched cookies do not revoke any stored row.

Why:

- sign-out should be safe to retry;
- logout should not create an account/session oracle;
- a tampered cookie should not be allowed to mutate a real session.

### 2. Keep all-session revocation for lifecycle actions, not normal logout

We added `revoke_user_session(session_id, revoked_at)` beside the existing `revoke_user_sessions(user_id, revoked_at)` store operation.

Why:

- normal logout has current-session semantics;
- deactivation, reset, and similar lifecycle actions can still revoke every session for a user;
- separate store methods make the scope explicit at call sites.

This was implemented across the trait, dynamic store dispatch, libsql store, and Postgres store. Store tests assert that revoking one session leaves another session for the same user active.

### 3. Share session-cookie resolution below user-focused auth checks

We split cookie parsing and session-row lookup into a small resolver that returns the raw token and stored session without requiring an active user.

Why:

- logout needs to revoke a session even when user resolution would fail;
- protected routes still need user-focused validation;
- shared parsing avoids drift in token-id extraction and hash checks.

Authenticated session lookup remains stricter: it rejects revoked, expired, hash-mismatched, missing-user, or disabled-user sessions and touches the session only after validation.

### 4. Make session cookie issue and expiry share one attribute contract

Session issue and expiry now share the same baseline attributes:

- `Path=/`
- `HttpOnly`
- `SameSite=Lax`
- conditional `Secure`

`Secure` is emitted when the effective forwarded scheme is HTTPS.

Why:

- a clearing cookie must target the same cookie scope as the issued cookie;
- local HTTP development should still work without a compatibility route;
- HTTPS deployments should get the stronger cookie attribute automatically.

### 5. Normalize pre-auth password-login failures

Password login now returns the same authentication failure for unknown email, wrong password, non-password accounts, non-admin accounts, and inactive accounts before a session exists.

Why:

- the login endpoint is pre-authenticated and should not disclose account state;
- platform-admin access is the only supported admin control-plane login scope;
- detailed lifecycle state remains available only through authenticated admin APIs.

### 6. Centralize admin route-auth semantics

The admin UI now has one auth-routing helper for:

- admin path normalization;
- the default signed-in path;
- redirect target construction;
- public admin route detection;
- platform-admin session checks.

Both the root route and child admin guards use that helper. Child guards remain in place so protected loaders do not run after an expired session.

Why:

- root and child guards need the same meaning for admin paths and redirect targets;
- protected loaders should still defend themselves;
- non-`platform_admin` sessions should be treated as unauthenticated for protected admin routes.

### 7. Put the sign-out transition in the shell and account rendering in the sidebar

`AppShell` owns the sign-out transition. `AppSidebar` owns the account menu rendering.

The sidebar footer now shows an account dropdown with:

- signed-in name;
- email;
- role;
- `Change password`;
- `Sign out`.

On successful sign-out, the shell calls `window.location.replace('/admin/login')`.

Why:

- the shell is the right place for app-level navigation after the HttpOnly cookie changes;
- replacing history avoids returning to a now-invalid protected page through browser back;
- the sidebar stays a rendering component instead of owning session mutation.

### 8. Keep the contract generated and tested

The logout endpoint is part of the `utoipa` admin API document, the checked-in OpenAPI artifact, and the generated TypeScript client.

Why:

- the admin UI should consume the same generated contract as the rest of the admin API;
- adding auth endpoints outside the contract would weaken the contract discipline established by earlier ADRs;
- E2E coverage now proves browser logout clears the session and redirects protected revisits to login.

## Consequences

Positive:

- operators now have an explicit sign-out path;
- normal logout has narrow current-session semantics;
- lifecycle/admin revocation and normal logout no longer share an ambiguous store operation;
- login failure behavior leaks less account state;
- route guard behavior is simpler to reason about and easier to reuse;
- stale preview shell copy has been removed;
- the admin UI account area now exposes the actions operators expect.

Tradeoffs:

- there is still no session-management UI for viewing or revoking other sessions;
- there is no "sign out everywhere" action in this issue;
- logout is intentionally quiet for invalid cookies, which means debugging cookie corruption requires server-side inspection rather than user-facing detail.

## Post-Implementation Review

The main design choice still looks right: current-session logout should be a server-side session revocation, not a client-only redirect or all-session invalidation. That makes the behavior narrow, understandable, and easy to extend later.

There are two things worth noting for future work:

- The effective-scheme logic for secure cookies is still a small helper inside the identity HTTP module. If more handlers need scheme-aware behavior, that should become a shared request-context helper rather than being copied.
- The admin account menu introduced a useful pattern for shell-level mutations. If more shell actions appear, we should keep mutation ownership in `AppShell` or a similarly high-level boundary instead of letting sidebar rendering components accumulate side effects.

What we should not do differently:

- Do not add a compatibility logout route.
- Do not add a client-only logout fallback.
- Do not make normal logout revoke every session.
- Do not preserve duplicate auth-routing helpers now that the shared helper exists.

The implementation deliberately removes stale shell copy and duplicated route helper logic rather than preserving old patterns. That is the right posture for the first iteration of the service.

## Follow-Up Work

- Add a dedicated session-management surface only when operators need to inspect or revoke other sessions.
- Add a separate "sign out everywhere" action only if product requirements call for it, backed by the existing all-session lifecycle revocation path.
- Keep OIDC redirect hardening with [issue #29](https://github.com/ahstn/oceans-llm/issues/29); it is adjacent auth work but not part of the logout/session lifecycle decision.
- Consider moving effective request-scheme detection into a shared HTTP helper if another code path needs it.

## Attribution

This ADR documents the implementation and post-implementation review of GitHub issue [#34](https://github.com/ahstn/oceans-llm/issues/34) and PR [#104](https://github.com/ahstn/oceans-llm/pull/104). It was prepared through collaborative human + AI implementation/design work.
