# ADR: Local SSO Compose Fixture and Browser Origin

- Date: 2026-05-15
- Status: Accepted
- Related Issues:
  - [#29: Replace development OIDC simulation with a standards-compliant provider flow](https://github.com/ahstn/oceans-llm/issues/29)
  - [#46: Research self-hosted OIDC/SSO tooling to unblock issue #29](https://github.com/ahstn/oceans-llm/issues/46)
  - [#64: Add declarative teams and users config to gateway.yaml](https://github.com/ahstn/oceans-llm/issues/64)
  - [#65: Refine declarative teams/users config for hardened SSO and OIDC identities](https://github.com/ahstn/oceans-llm/issues/65)
- Related Docs:
  - [OIDC and SSO](../access/oidc-and-sso-status.md)
  - [Testing Authentication Locally](../development/authentication-testing.md)
  - [ADR: Authentik Local SSO Test IdP](2026-05-15-authentik-local-sso-test-idp.md)

## Context

The OIDC implementation moved from a development simulation to a real authorization-code flow. That changed the local test requirement. It is no longer enough for the login screen to render a provider button; the local stack must exercise the browser redirect, provider discovery, code exchange, callback, JIT policy, and existing `ogw_session` cookie issuance.

The first local attempt reused [../../deploy/config/gateway.yaml](../../deploy/config/gateway.yaml) from [../../compose.local.yaml](../../compose.local.yaml). That was the wrong coupling. The deploy config is intentionally conservative: the Authentik provider is present but disabled, and JIT is disabled by default. Local SSO testing needs the opposite shape: an enabled provider, explicit local JIT defaults, and predictable fixture credentials.

The second issue was URL ownership. The local admin UI is reachable in two ways:

- through the gateway at `http://localhost:8080/admin`
- directly on the admin UI container at `http://localhost:3003/admin`

OIDC start URLs are gateway API URLs, not admin UI URLs. When the login page generated a root-relative `/api/v1/auth/oidc/start` link under the TanStack `/admin` base path, direct UI access could resolve it as `/admin/api/v1/auth/oidc/start` on port `3003`. That produced redirect loops instead of entering the IdP flow.

## Decision

Use a local-only gateway config for the source-built compose stack:

- [../../compose.local.yaml](../../compose.local.yaml) mounts [../../deploy/config/gateway.local.yaml](../../deploy/config/gateway.local.yaml)
- [../../deploy/config/gateway.local.yaml](../../deploy/config/gateway.local.yaml) enables the Authentik provider
- local JIT is enabled explicitly and assigns the configured `platform_admin` role and `platform` team admin membership
- [../../deploy/config/gateway.yaml](../../deploy/config/gateway.yaml) remains deploy-oriented and disabled by default

Keep the browser-facing gateway URL explicit in the admin UI:

- [../../compose.local.yaml](../../compose.local.yaml) sets `ADMIN_GATEWAY_BROWSER_ORIGIN` to `http://localhost:8080`
- [../../crates/admin-ui/web/src/server/gateway-client.server.ts](../../crates/admin-ui/web/src/server/gateway-client.server.ts) separates server-to-gateway origin resolution from browser-to-gateway origin resolution
- [../../crates/admin-ui/web/src/routes/login.tsx](../../crates/admin-ui/web/src/routes/login.tsx) builds SSO start links from the browser-facing gateway origin

Keep Authentik browser-visible on localhost:

- Authentik HTTP is exposed as `http://localhost:9000`
- the fixture issuer remains `http://authentik.localhost:9000/application/o/oceans-llm/`
- the callback remains `http://localhost:8080/api/v1/auth/oidc/callback`

## Rationale

Local development and deploy configuration have different risk profiles.

The local stack is a disposable fixture. It should make the complete SSO path easy to exercise without copying config, editing provider flags, or guessing passwords. Enabling Authentik and JIT there reduces setup ambiguity and makes regressions visible quickly.

The deploy config is a reusable deployment starting point. Enabling SSO and JIT there would create a surprising access path unless the maintainer has already supplied the IdP secret, issuer, public base URL, and desired role policy. Keeping the provider disabled by default preserves that boundary.

Browser origins need to be modeled separately from container origins. The admin UI server can reach the gateway at `http://gateway:8080` inside Docker, but a browser cannot. Conversely, the browser can use `http://localhost:8080`, which is the correct public gateway origin for local OIDC redirects. Treating those as one setting made direct `3003` testing brittle.

## Consequences

Local manual SSO testing now has a stable path:

1. `docker compose --profile sso -f compose.local.yaml up --build`
2. open `http://localhost:8080/admin/login` or `http://localhost:3003/admin/login`
3. choose `Sign in with Authentik`
4. authenticate as `sso-user@example.com` with `sso-user-password`

The same source-built stack can still test password login:

- fresh local gateway database: `admin@local` / `admin`
- older local volumes keep the bootstrap password that was first seeded

The split config means docs and review should check two files:

- local fixture behavior: [../../deploy/config/gateway.local.yaml](../../deploy/config/gateway.local.yaml)
- deploy-safe default behavior: [../../deploy/config/gateway.yaml](../../deploy/config/gateway.yaml)

## Alternatives Considered

One option was to keep using `deploy/config/gateway.yaml` and tell maintainers to edit it locally. That keeps fewer files, but it makes local SSO testing stateful and easy to misread in review. It also invites accidental deploy config changes.

Another option was to tell maintainers to use only `http://localhost:8080/admin` and ignore direct `3003` access. That does not match the compose file, which exposes the admin UI port for local inspection. If the port exists, the auth links should behave correctly.

Using `authentik-server:9000` as the issuer would make container-to-container discovery simple, but it would emit provider endpoints a browser cannot resolve. The local fixture uses a browser-resolvable issuer and exposes Authentik on localhost instead.

## Validation

The implementation was validated with:

```shell
docker compose --profile sso -f compose.local.yaml config --quiet
bun test --cwd crates/admin-ui/web src/server/gateway-client.server.test.ts
mise run lint
```

The running local stack was also checked by confirming:

- `/api/v1/auth/oidc/providers` returns the enabled `authentik` provider
- `/api/v1/auth/oidc/start?provider_key=authentik&redirect_to=/admin` returns a `307` to Authentik
- the login route rendered through `localhost:3003` receives `startOrigin: "http://localhost:8080"`
