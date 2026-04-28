# Deploy

`Owns`: the checked-in deploy entry points under `deploy/`.
`Depends on`: [../README.md](../README.md), [../docs/setup/deploy-and-operations.md](../docs/setup/deploy-and-operations.md)
`See also`: [../docs/setup/runtime-bootstrap-and-access.md](../docs/setup/runtime-bootstrap-and-access.md), [../docs/setup/kubernetes-and-helm.md](../docs/setup/kubernetes-and-helm.md), [../docs/operations/operator-runbooks.md](../docs/operations/operator-runbooks.md)

This directory contains the checked-in compose quick start and the supported Helm chart.

## Files

- `compose.yaml`
  - gateway, admin UI, and PostgreSQL
- `config/gateway.yaml`
  - mounted production-shaped gateway config
- `.env.example`
  - image tags and secret inputs
- `helm/oceans-llm`
  - Helm chart for Kubernetes installs

## Compose Usage

```bash
cp deploy/.env.example deploy/.env
docker compose -f deploy/compose.yaml up -d
```

Default endpoint:

- gateway and admin UI:
  - `http://localhost:8080`

## Helm Usage

The supported chart is published as:

```bash
oci://ghcr.io/ahstn/charts/oceans-llm
```

Local chart sources live in [helm/oceans-llm](helm/oceans-llm/README.md).

```bash
helm install oceans-llm oci://ghcr.io/ahstn/charts/oceans-llm \
  --version <version> \
  --values values.yaml
```

Use [Kubernetes and Helm](../docs/setup/kubernetes-and-helm.md) for the chart contract, secret modes, database modes, and scheduling controls.

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

- Kubernetes and Helm:
  - [Kubernetes and Helm](../docs/setup/kubernetes-and-helm.md)
- startup and first access:
  - [Runtime Bootstrap and Access](../docs/setup/runtime-bootstrap-and-access.md)
- topology and caveats:
  - [Deploy and Operations](../docs/setup/deploy-and-operations.md)
- action-oriented recovery:
  - [Admin Runbooks](../docs/operations/operator-runbooks.md)
- config contract:
  - [Configuration Reference](../docs/configuration/configuration-reference.md)

Use [Documentation Home](../docs/index.md) when you need the full admin, user, and maintainer map.
