# Deploy Compose

`Owns`: the GHCR-based compose quick start in `deploy/`.
`Depends on`: [../README.md](../README.md), [../docs/deploy-and-operations.md](../docs/deploy-and-operations.md)
`See also`: [../docs/runtime-bootstrap-and-access.md](../docs/runtime-bootstrap-and-access.md), [../docs/operator-runbooks.md](../docs/operator-runbooks.md)

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

## What Gets Created

The checked-in stack creates:

- a PostgreSQL container
- a gateway container
- an admin UI container
- config-seeded runtime objects on gateway boot

The gateway can run migrations and seed config-backed objects on startup.

## Access After Boot

The checked-in compose path guarantees:

- data-plane access through the seeded `GATEWAY_API_KEY`

The checked-in compose path does not guarantee:

- a bootstrap admin user
- a first-login password flow
- hardened OIDC or SSO bootstrap

Admin access depends on the mounted runtime config and any existing admin rows in the database.

## What This Quick Start Does Not Configure

- no checked-in OTLP collector
- no checked-in bootstrap-admin block
- no declarative users, teams, or budgets

That missing declarative identity path is part of the future config-as-code direction tracked in [issue #64](https://github.com/ahstn/oceans-llm/issues/64) and [issue #65](https://github.com/ahstn/oceans-llm/issues/65).

## Follow The Canonical Docs

Use the quick start here, then switch to the canonical pages for the rest:

- startup and first access:
  - [Runtime Bootstrap and Access](../docs/runtime-bootstrap-and-access.md)
- topology and caveats:
  - [Deploy and Operations](../docs/deploy-and-operations.md)
- action-oriented recovery:
  - [Operator Runbooks](../docs/operator-runbooks.md)
- config contract:
  - [Configuration Reference](../docs/configuration-reference.md)
