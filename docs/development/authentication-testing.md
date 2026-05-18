# Testing Authentication Locally

`See also`: [OIDC and SSO](../access/oidc-and-sso-status.md), [Identity and Access](../access/identity-and-access.md), [Runtime Bootstrap and Access](../setup/runtime-bootstrap-and-access.md), [ADR: Local SSO Compose Fixture and Browser Origin](../adr/2026-05-15-local-sso-compose-fixture-and-browser-origin.md)

This page is for maintainers testing admin authentication from the source-built Docker Compose stack.

## Local Compose Stack

Run the source-built stack with the SSO profile:

```shell
docker compose --profile sso -f compose.local.yaml up --build
```

The stack builds local gateway and admin UI images from the working tree.

Important files:

- [../../compose.local.yaml](../../compose.local.yaml): local Docker topology
- [../../deploy/config/gateway.local.yaml](../../deploy/config/gateway.local.yaml): gateway config mounted by local compose
- [../../deploy/authentik/oceans-llm-blueprint.yaml](../../deploy/authentik/oceans-llm-blueprint.yaml): Authentik fixture application, provider, and test user
- [../../deploy/config/gateway.yaml](../../deploy/config/gateway.yaml): deploy-oriented config, not used by `compose.local.yaml`

## URLs

Use these URLs when testing the local stack:

| Purpose | URL |
| --- | --- |
| Gateway and normal admin entrypoint | `http://localhost:8080` |
| Admin login through the gateway | `http://localhost:8080/admin/login` |
| Direct admin UI container port | `http://localhost:3003/admin/login` |
| Authentik | `http://localhost:9000` |
| Authentik issuer | `http://authentik.localhost:9000/application/o/oceans-llm/` |
| OIDC callback | `http://localhost:8080/api/v1/auth/oidc/callback` |
| Public provider list | `http://localhost:8080/api/v1/auth/oidc/providers` |
| OIDC start endpoint | `http://localhost:8080/api/v1/auth/oidc/start?provider_key=authentik&redirect_to=/admin` |

The direct admin UI port is useful for inspecting the UI container. SSO still starts through the gateway on port `8080`, because the gateway owns the OIDC transaction state and callback.

## Fixture Credentials

Local defaults are intentionally simple and only belong to the disposable compose fixture.

| Account or secret | Value |
| --- | --- |
| Fresh local bootstrap admin | `admin@local` / `admin` |
| Authentik admin | `akadmin@example.com` / `akadmin-password` |
| Authentik SSO test user | `sso-user@example.com` / `sso-user-password` |
| OIDC client id | `oceans-llm` |
| OIDC client secret | `oceans-llm-local-secret` |
| Gateway API key | `gwk_localdev.replace-me` |

Existing Docker volumes keep the bootstrap admin password that was first seeded. If an older volume was created with `change-me-admin`, that password remains valid until the local gateway database is recreated or the password is changed through the product flow.

## SSO Test Flow

1. Start the stack:

```shell
docker compose --profile sso -f compose.local.yaml up --build
```

2. Open `http://localhost:8080/admin/login`.
3. Select `Sign in with Authentik`.
4. Sign in to Authentik as `sso-user@example.com` with `sso-user-password`.
5. Confirm the browser returns to `/admin` with an `ogw_session` cookie.

The same test can start from `http://localhost:3003/admin/login`. The login page should still send the browser to `http://localhost:8080/api/v1/auth/oidc/start`, not to `/admin/api/...` on port `3003`.

## Password Login Test Flow

Use password login when testing the bootstrap session path without SSO.

1. Open `http://localhost:8080/admin/login`.
2. Sign in as `admin@local`.
3. Use `admin` on a fresh local database, or the password that was seeded into the existing local volume.
4. If password rotation is required, complete the rotation and continue to `/admin`.

The bootstrap admin is created only when no platform admin already exists. Changing `GATEWAY_BOOTSTRAP_ADMIN_PASSWORD` after the first seed does not rewrite an existing local password.

## Authentik Configuration

The local fixture uses Authentik `2025.4.4` by default through `AUTHENTIK_TAG`.

The OIDC provider shape in [../../deploy/config/gateway.local.yaml](../../deploy/config/gateway.local.yaml) is:

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

The matching Authentik blueprint sets:

- application: `Oceans LLM`
- client id: `oceans-llm`
- client secret: `oceans-llm-local-secret`
- redirect URI: `http://localhost:8080/api/v1/auth/oidc/callback`
- subject mode: hashed user id
- scopes: `openid`, `email`, `profile`

## Troubleshooting

No SSO button appears:

- confirm the stack uses [../../deploy/config/gateway.local.yaml](../../deploy/config/gateway.local.yaml)
- check `http://localhost:8080/api/v1/auth/oidc/providers`
- restart `gateway` after changing provider config

The SSO button links to `/admin/api/v1/auth/oidc/start`:

- rebuild the admin UI image
- confirm `ADMIN_GATEWAY_BROWSER_ORIGIN` is set to `http://localhost:8080`
- prefer `http://localhost:8080/admin/login` for the main manual flow

Authentik rejects the callback:

- confirm the callback URL is `http://localhost:8080/api/v1/auth/oidc/callback`
- confirm `GATEWAY_PUBLIC_BASE_URL` is `http://localhost:8080`
- confirm the client secret matches `AUTHENTIK_OCEANS_LLM_CLIENT_SECRET`

Password login does not accept `admin`:

- check whether the local Postgres volume predates the current fixture
- try the previously seeded local password
- recreate the local database volume only when local data can be discarded
