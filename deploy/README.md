# Deploy Compose

`Owns`: the GHCR-based compose quick start in `deploy/`.
`Depends on`: [../README.md](../README.md), [../docs/deploy-and-operations.md](../docs/setup/deploy-and-operations.md)
`See also`: [../docs/runtime-bootstrap-and-access.md](../docs/setup/runtime-bootstrap-and-access.md), [../docs/admin-runbooks.md](../docs/operations/operator-runbooks.md)

This directory is the quick start for the checked-in compose deploy path.

## Files

- `compose.yaml`
  - gateway, admin UI, and PostgreSQL
- `config/gateway.yaml`
  - mounted production-shaped gateway config
- `.env.example`
  - image tags and secret inputs

## Usage

```bash
cp deploy/.env.example deploy/.env
docker compose -f deploy/compose.yaml up -d
```

Default endpoint:

- gateway and admin UI:
  - `http://localhost:8080`

## Required Environment

Set these values in `deploy/.env` before treating the stack as usable:

- `POSTGRES_DB`
- `POSTGRES_USER`
- `POSTGRES_PASSWORD`
- `GATEWAY_API_KEY`
- `GATEWAY_BOOTSTRAP_ADMIN_PASSWORD`
- `OPENAI_API_KEY`
- `OPENAI_API_KEY_SECONDARY`

The mounted gateway config references both OpenAI keys and the bootstrap admin password through env-backed secrets. Empty provider keys can let the containers start, but provider-backed requests will fail when those routes execute.

## What Gets Created

The checked-in stack creates:

- a PostgreSQL container
- a gateway container
- an admin UI container
- config-seeded runtime objects on gateway boot, including teams, invited users, and budgets

The gateway can run migrations and seed config-backed objects on startup.

## Access After Boot

The checked-in compose path guarantees:

- data-plane access through the seeded `GATEWAY_API_KEY`
- a bootstrap admin at `admin@local` once `GATEWAY_BOOTSTRAP_ADMIN_PASSWORD` is set
- the example seeded teams, users, and budgets from `config/gateway.yaml`

The checked-in compose path does not guarantee:

- hardened OIDC or SSO bootstrap

Seeded password and OIDC users are created as invited identities. Admins still generate onboarding links from the admin UI when those accounts need to sign in.

## Deployment Verification

After boot, run a short readiness pass before sending real traffic:

- confirm the containers are healthy:

```bash
docker compose -f deploy/compose.yaml ps
```

- confirm the gateway responds:

```bash
curl --fail http://localhost:8080/healthz
curl --fail http://localhost:8080/readyz
```

- confirm the seeded API key can see gateway models:

```bash
curl --fail \
  -H "Authorization: Bearer ${GATEWAY_API_KEY}" \
  http://localhost:8080/v1/models
```

- sign in to `/admin` as `admin@local` with `GATEWAY_BOOTSTRAP_ADMIN_PASSWORD`
- confirm provider-backed traffic with one low-cost `/v1/*` request
- inspect gateway logs if config seeding, provider auth, or route viability fails

## What This Quick Start Does Not Configure

- no checked-in OTLP collector
- no hardened SSO-first bootstrap path

Hardened declarative SSO-backed identity matching remains tracked in [issue #65](https://github.com/ahstn/oceans-llm/issues/65).

## Follow The Canonical Docs

Use the quick start here, then switch to the canonical pages for the rest:

- startup and first access:
  - [Runtime Bootstrap and Access](../docs/setup/runtime-bootstrap-and-access.md)
- topology and caveats:
  - [Deploy and Operations](../docs/setup/deploy-and-operations.md)
- action-oriented recovery:
  - [Admin Runbooks](../docs/operations/operator-runbooks.md)
- config contract:
  - [Configuration Reference](../docs/configuration/configuration-reference.md)

Use [Documentation Home](../docs/index.md) when you need the full admin, user, and maintainer map.
