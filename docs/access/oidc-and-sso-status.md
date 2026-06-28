# OIDC and SSO

`See also`: [Identity and Access](identity-and-access.md), [GitHub OAuth SSO Setup for Admins](github-oauth-admin-setup.md), [Testing Authentication Locally](../development/authentication-testing.md), [Runtime Bootstrap and Access](../setup/runtime-bootstrap-and-access.md), [Configuration Reference](../configuration/configuration-reference.md), [Deploy and Operations](../setup/deploy-and-operations.md), [Admin Control Plane](admin-control-plane.md), [ADR: Identity Foundation for Users, Teams, and API Key Ownership](../adr/2026-03-05-identity-foundation.md), [ADR: Authentik Local SSO Test IdP](../adr/2026-05-15-authentik-local-sso-test-idp.md), [ADR: Local SSO Compose Fixture and Browser Origin](../adr/2026-05-15-local-sso-compose-fixture-and-browser-origin.md)

Oceans LLM supports OIDC and OAuth SSO for the admin control plane. The browser ends each flow with the same `ogw_session` HttpOnly cookie used by password login.

## Runtime Contract

The OIDC/OAuth flow includes:

- `auth.oidc.providers` in `gateway.yaml` seeds enabled OIDC providers
- `/api/v1/auth/oidc/providers` exposes enabled OIDC providers for `/admin/login`
- `/api/v1/auth/oauth/providers` exposes enabled OAuth providers for `/admin/login`
- `/api/v1/auth/oidc/start` performs provider discovery and redirects with state, nonce, and PKCE
- `/api/v1/auth/oidc/callback` consumes one-time state, exchanges the authorization code, verifies the ID token and nonce, and issues the existing `ogw_session` cookie
- `/api/v1/auth/oauth/start` redirects to the OAuth provider with one-time state and PKCE
- `/api/v1/auth/oauth/callback/github` consumes one-time state, exchanges the code with GitHub, resolves numeric subject plus the selected primary email, and issues `ogw_session`
- invited/config-declared OIDC users activate on first successful provider login
- provider-specific JIT user creation can assign explicit global role, team membership, and request logging defaults
- direct GitHub OAuth requires a GitHub-verified primary email by default, can use `sso_email_verification_enabled: false` as an admin escape hatch, and can restrict sign-in and JIT provisioning to configured email domains
- local Authentik compose profiles provide a repeatable manual IdP fixture

## Security Boundary

The current flow preserves the same-origin admin session cookie model. Successful SSO creates the existing HttpOnly `ogw_session` cookie and redirects back into `/admin`.

Account linking is intentionally conservative:

- existing `(provider, sub)` links win
- invited/config-declared OIDC users with matching normalized email and provider link are activated and linked
- unmatched identities use the provider's explicit JIT policy
- GitHub OAuth `sso_email_verification_enabled` and `allowed_email_domains` are enforced before account linking, invite activation, JIT creation, or session issuance
- existing password/local users with the same email are rejected instead of auto-linked

## Practical Admin Impact

Admins should assume these boundaries:

- password login remains available unless admins remove or disable those users
- JIT defaults are provider policy, not provider claims mapping
- no user becomes `platform_admin` unless the provider config explicitly says so
- email-only matching never links an existing password user to SSO

## Local Test IdP

The repo ships an opt-in Authentik fixture for local/manual SSO testing:

- [../../compose.local.yaml](../../compose.local.yaml) includes the `sso` profile for source-built local runs.
- [../../deploy/config/gateway.local.yaml](../../deploy/config/gateway.local.yaml) enables the local Authentik provider and JIT policy for that compose path.
- [../../deploy/compose.yaml](../../deploy/compose.yaml) includes the same `sso` profile for image-based deploy runs.
- [../../deploy/authentik/oceans-llm-blueprint.yaml](../../deploy/authentik/oceans-llm-blueprint.yaml) creates the `Oceans LLM` OIDC application and the `sso-user@example.com` test user.
- [../development/authentication-testing.md](../development/authentication-testing.md) owns the local testing procedure, URLs, and fixture passwords.

Run it with:

```shell
docker compose --profile sso -f compose.local.yaml up --build
```

The fixture defaults are:

- Authentik URL: `http://localhost:9000`
- Authentik admin: `akadmin@example.com` / `akadmin-password`
- SSO test user: `sso-user@example.com` / `sso-user-password`
- local bootstrap admin on a fresh gateway database: `admin@local` / `admin`
- OIDC client id: `oceans-llm`
- OIDC client secret: `oceans-llm-local-secret`
- tested Authentik version: `2025.4.4`

Existing local Docker volumes keep the bootstrap admin password that was first seeded. If the volume was created before this fixture used `admin`, sign in with the old configured password or recreate the local gateway database volume when it is safe to discard local data.

The local compose config seeds an enabled Authentik provider for manual testing. The checked-in deploy config also defines the provider, but leaves it disabled until a maintainer opts in for the target environment.

## Authentik Provider Shape

The local Authentik provider uses this gateway config shape:

```yaml
auth:
  oidc:
    public_base_url: env.GATEWAY_PUBLIC_BASE_URL
    providers:
      - key: authentik
        label: Authentik
        issuer_url: http://authentik.localhost:9000/application/o/oceans-llm/
        client_id: oceans-llm
        client_secret: env.AUTHENTIK_OCEANS_LLM_CLIENT_SECRET
        scopes:
          - openid
          - email
          - profile
        enabled: true
        jit:
          enabled: true
          global_role: platform_admin
          request_logging_enabled: true
          membership:
            team: platform
            role: admin
```

The matching local Authentik application must use:

- provider key: `authentik`
- issuer URL: `http://authentik.localhost:9000/application/o/oceans-llm/`
- callback URL: `http://localhost:8080/api/v1/auth/oidc/callback`
- client id: `oceans-llm`
- public base URL env ref: `GATEWAY_PUBLIC_BASE_URL`
- client secret env ref: `AUTHENTIK_OCEANS_LLM_CLIENT_SECRET`
- local JIT: enabled with `platform_admin`, team `platform` / `admin`
- deploy JIT: disabled by default and must be enabled explicitly per environment

## Manual Validation

1. Start the stack: `docker compose --profile sso -f compose.local.yaml up --build`.
2. Open `http://localhost:8080/admin/login`.
3. Choose `Sign in with Authentik`.
4. Log in as `sso-user@example.com` / `sso-user-password`.
5. Confirm the browser returns to `/admin` with an `ogw_session` cookie.

## Current Boundaries

- provider policy owns JIT defaults
- group or claim-to-role mapping is outside the current contract
- Okta is a later benchmark provider, not the local fixture
- discovery and JWKS metadata are fetched during login
- Authentik browser automation is still manual-first

## What This Page Does Not Own

- user lifecycle and team rules: [identity-and-access.md](identity-and-access.md)
- config field syntax for providers: [configuration-reference.md](../configuration/configuration-reference.md)
- local auth test procedure: [authentication-testing.md](../development/authentication-testing.md)
- admin UI capability map: [admin-control-plane.md](admin-control-plane.md)
- deploy topology and first-access behavior: [deploy-and-operations.md](../setup/deploy-and-operations.md), [runtime-bootstrap-and-access.md](../setup/runtime-bootstrap-and-access.md)
