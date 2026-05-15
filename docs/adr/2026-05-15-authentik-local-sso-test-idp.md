# ADR: Authentik Local SSO Test IdP

- Date: 2026-05-15
- Status: Accepted
- Related Issues:
  - [#46](https://github.com/ahstn/oceans-llm/issues/46)
  - [#65](https://github.com/ahstn/oceans-llm/issues/65)
  - [#29](https://github.com/ahstn/oceans-llm/issues/29)

## Context

Issue #46 asks for a Docker-friendly SSO/OIDC tool that can serve as a realistic local IdP and eventually broker upstream providers. Issue #65 then depends on the final OIDC identity contract so SSO-backed users, teams, roles, and budgets can be reconciled from `gateway.yaml` without duplicate local identities.

The current product has OIDC identity storage, admin UI provider selectors, invited OIDC users, provider seeding, and a standards-based authorization-code callback flow.

## Decision

Use Authentik as the preferred local/manual SSO test IdP.

The checked-in compose files now include an opt-in `sso` profile with:

- `authentik-server`
- `authentik-worker`
- `authentik-postgres`
- `authentik-redis`

The shared fixture blueprint at [../../deploy/authentik/oceans-llm-blueprint.yaml](../../deploy/authentik/oceans-llm-blueprint.yaml) creates:

- Authentik application: `Oceans LLM`
- OIDC provider client id: `oceans-llm`
- OIDC provider client secret: `oceans-llm-local-secret`
- redirect URI: `http://localhost:8080/api/v1/auth/oidc/callback`
- test user: `sso-user@example.com` / `sso-user-password`

Authentik bootstrap admin is configured through compose environment:

- `AUTHENTIK_BOOTSTRAP_EMAIL`, default `akadmin@example.com`
- `AUTHENTIK_BOOTSTRAP_PASSWORD`, default `akadmin-password`
- `AUTHENTIK_BOOTSTRAP_TOKEN`, default `akadmin-bootstrap-token`

## Why Authentik

Authentik is a strong fit for this repo because it has official Docker Compose setup, supports OAuth2/OIDC provider behavior, supports file-based blueprints for repeatable local configuration, and can broker social/federated sources later. That maps well to the manual test workflow needed before hardened OIDC is declared complete.

Key evidence from current Authentik docs:

- Docker Compose install is a supported test/small-production path.
- Automated install supports bootstrap password, token, and email environment variables on the worker.
- Blueprints are YAML-based config as code and are auto-discovered from `/blueprints`.
- Blueprint-created users can include a password field.
- OAuth2/OIDC providers support the authorization-code flow, redirect URIs, client id, client secret, scopes, and ID-token claims.

## Alternatives Considered

ZITADEL and Keycloak remain credible follow-up validation targets. ZITADEL has strong identity-brokering docs and Keycloak is mature and widely deployed, but both add more weight than we need for the first local/manual fixture.

Dex is a useful lightweight federated provider, but it is less representative of the operator-facing IdP administration and app/provider configuration path we want to test.

Tinyauth is promising for a very small setup, but it is too narrow as the primary local fixture for enterprise SSO semantics.

Authelia, Pocket ID, and Kanidm are weaker fits for this specific test role because the current requirement emphasizes upstream brokering and realistic OIDC app/provider management.

Okta should remain a benchmark/manual validation target, not the default local tool.

## Implemented Contract

Oceans LLM now performs OIDC authorization-code login with discovery, PKCE, one-time hashed state, nonce storage, code exchange, ID-token verification, and existing `ogw_session` cookie issuance.

Provider rows can be seeded from `auth.oidc.providers` in `gateway.yaml`. Each provider defines label, issuer URL, client id, client secret ref, scopes, enabled flag, and explicit JIT defaults.

Subject semantics:

- durable links are keyed by `(oidc_provider_id, subject)`
- the subject is the provider `sub` claim, not an email-derived value
- matching by email is only used to activate predeclared invited OIDC users for the same provider
- existing password/local users are never auto-linked by matching email

JIT behavior:

- disabled by default
- provider-specific when enabled
- assigns only the configured global role and optional configured team membership
- does not perform group/claim mapping in this phase

## Follow-Up Tasks

1. Add automated browser coverage against the Authentik fixture.
2. Add discovery/JWKS caching if login latency or provider rate limits become a practical problem.
3. Add group/claim-to-role mapping as a separate policy feature.
4. Validate Okta manually as a benchmark provider after Authentik.
