# OIDC and SSO Status

`See also`: [Identity and Access](identity-and-access.md), [Runtime Bootstrap and Access](../setup/runtime-bootstrap-and-access.md), [Configuration Reference](../configuration/configuration-reference.md), [Deploy and Operations](../setup/deploy-and-operations.md), [Admin Control Plane](admin-control-plane.md), [ADR: Identity Foundation for Users, Teams, and API Key Ownership](../adr/2026-03-05-identity-foundation.md), [ADR: Authentik Local SSO Test IdP](../adr/2026-05-15-authentik-local-sso-test-idp.md)

This page tracks the OIDC/SSO implementation boundary. Oceans LLM now supports a real OIDC authorization-code login path and an Authentik local fixture, with explicit provider-configured JIT defaults.

## Current State

The live product supports pre-provisioned OIDC users and OIDC-flavored onboarding flows in the admin control plane.

What exists today:

- `auth.oidc.providers` in `gateway.yaml` seeds enabled OIDC providers
- `/api/v1/auth/oidc/providers` exposes enabled SSO providers for `/admin/login`
- `/api/v1/auth/oidc/start` performs provider discovery and redirects to the IdP with state, nonce, and PKCE
- `/api/v1/auth/oidc/callback` consumes one-time state, exchanges the authorization code, verifies the ID token and nonce, and issues the existing `ogw_session` cookie
- invited/config-declared OIDC users activate on first successful provider login
- provider-specific JIT user creation can assign explicit global role, team membership, and request logging defaults
- local Authentik compose profiles provide a repeatable manual IdP fixture

## Security Boundary

The current flow preserves the same-origin admin session cookie model. Successful SSO creates the existing HttpOnly `ogw_session` cookie and redirects back into `/admin`.

Account linking is intentionally conservative:

- existing `(provider, sub)` links win
- invited/config-declared OIDC users with matching normalized email and provider link are activated and linked
- unmatched identities use the provider's explicit JIT policy
- existing password/local users with the same email are rejected instead of auto-linked

## Practical Admin Impact

Admins should assume these boundaries:

- password login remains available unless operators remove or disable those users
- JIT defaults are provider policy, not provider claims mapping
- no user becomes `platform_admin` unless the provider config explicitly says so
- email-only matching never links an existing password user to SSO

## Local Test IdP

The repo now ships an opt-in Authentik fixture for local/manual SSO testing:

- [../../compose.local.yaml](../../compose.local.yaml) includes the `sso` profile for source-built local runs.
- [../../deploy/compose.yaml](../../deploy/compose.yaml) includes the same `sso` profile for image-based deploy runs.
- [../../deploy/authentik/oceans-llm-blueprint.yaml](../../deploy/authentik/oceans-llm-blueprint.yaml) creates the `Oceans LLM` OIDC application and the `sso-user@example.com` test user.

Run it with:

```shell
docker compose --profile sso -f compose.local.yaml up
```

The fixture defaults are:

- Authentik URL: `http://localhost:9000`
- Authentik admin: `akadmin@example.com` / `akadmin-password`
- SSO test user: `sso-user@example.com` / `sso-user-password`
- OIDC client id: `oceans-llm`
- OIDC client secret: `oceans-llm-local-secret`

The checked-in deploy config seeds an Authentik provider:

- provider key: `authentik`
- issuer URL: `http://authentik.localhost:9000/application/o/oceans-llm/`
- callback URL: `http://localhost:8080/api/v1/auth/oidc/callback`
- client id: `oceans-llm`
- client secret env ref: `AUTHENTIK_OCEANS_LLM_CLIENT_SECRET`, default `oceans-llm-local-secret`
- JIT: enabled, explicitly `platform_admin`, team `platform` / `admin` for local manual validation

Manual validation:

1. Start the stack: `docker compose --profile sso -f compose.local.yaml up`.
2. Run migrations and seed config if the gateway did not do so during startup.
3. Open `http://localhost:8080/admin/login`.
4. Choose `Sign in with Authentik`.
5. Log in as `sso-user@example.com` / `sso-user-password`.

## Current Gaps

- claim/group-to-role mapping is not implemented
- Okta validation remains a manual follow-up benchmark
- discovery and JWKS metadata are fetched on login; a runtime cache can be added later
- automated end-to-end browser coverage against Authentik is still manual-first

## What This Page Does Not Own

- user lifecycle and team rules: [identity-and-access.md](identity-and-access.md)
- config field syntax for providers: [configuration-reference.md](../configuration/configuration-reference.md)
- admin UI capability map: [admin-control-plane.md](admin-control-plane.md)
- deploy topology and first-access behavior: [deploy-and-operations.md](../setup/deploy-and-operations.md), [runtime-bootstrap-and-access.md](../setup/runtime-bootstrap-and-access.md)
