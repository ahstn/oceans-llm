# Runtime Bootstrap and Access

`See also`: [Configuration Reference](../configuration/configuration-reference.md), [Identity and Access](../access/identity-and-access.md), [Oceans LLM Gateway](../../README.md), [Deploy Compose](../../deploy/README.md), [Deploy and Operations](deploy-and-operations.md), [Operator Runbooks](../operations/operator-runbooks.md)

This page explains what the gateway does when it starts and what access exists right after boot.

## Source of Truth

- CLI entry points: [../crates/gateway/src/main.rs](../../crates/gateway/src/main.rs)
- Startup scripts: [../scripts/start-dev-stack.sh](../../scripts/start-dev-stack.sh), [../scripts/start-prod.sh](../../scripts/start-prod.sh)
- Runtime commands: [../mise.toml](../../mise.toml)
- Checked-in configs: [../gateway.yaml](../../gateway.yaml), [../gateway.prod.yaml](../../gateway.prod.yaml), [../deploy/config/gateway.yaml](../../deploy/config/gateway.yaml)

## Startup Actions

The normal startup path is `gateway serve`.

At startup, the gateway can do three separate things:

- run migrations
- seed config-backed objects such as providers, models, and seed API keys
- ensure a bootstrap admin exists

Those actions are controlled by:

- `GATEWAY_RUN_MIGRATIONS`
- `GATEWAY_SEED_CONFIG`
- `GATEWAY_BOOTSTRAP_ADMIN`

The same behavior is also exposed through explicit commands:

- `gateway migrate`
- `gateway seed-config`
- `gateway bootstrap-admin`

## Startup Shapes

### Local development

- config: [../gateway.yaml](../../gateway.yaml)
- database: libsql or SQLite
- usual entry point: [../scripts/start-dev-stack.sh](../../scripts/start-dev-stack.sh)
- bootstrap admin: enabled
- forced password change: off
- seed API keys: driven by config

What exists after boot:

- a gateway on `http://localhost:8080`
- an admin UI at `http://localhost:8080/admin`
- a bootstrap admin at `admin@local` with password `admin`

### Production-shaped local run

- config: [../gateway.prod.yaml](../../gateway.prod.yaml)
- database: PostgreSQL
- usual entry point: [../scripts/start-prod.sh](../../scripts/start-prod.sh)
- bootstrap admin: enabled
- forced password change: on
- seed API keys: driven by config

What exists after boot:

- the same admin email and password as the local-dev config
- a forced password rotation on first sign-in
- a runtime shape closer to deploy and release behavior

### GHCR compose deploy

- compose entry point: [../deploy/compose.yaml](../../deploy/compose.yaml)
- mounted config: [../deploy/config/gateway.yaml](../../deploy/config/gateway.yaml)
- database: PostgreSQL
- bootstrap admin: not defined in the checked-in deploy config
- seeded API key: defined through `GATEWAY_API_KEY`

What exists after boot:

- API access through the seeded gateway API key
- no checked-in bootstrap admin guarantee
- admin access only if the mounted runtime config enables bootstrap admin or a prior admin already exists

## Bootstrap Admin

Bootstrap admin creation is for control-plane access.

- It is separate from seeded API keys.
- It is checked at startup when bootstrap behavior is enabled.
- It can also be created with `gateway bootstrap-admin`.

Checked-in defaults:

- local config keeps the bootstrap admin on and does not force password rotation
- production-shaped local config keeps the bootstrap admin on and forces password rotation

For the lifecycle and ownership rules after that first sign-in, see [identity-and-access.md](../access/identity-and-access.md).

## Seeded API Keys

Seeded API keys are for data-plane access.

- They come from the config seed path.
- They are useful for deploy-style automation and test-style setup.
- They do not replace admin login.

The compose deploy path leans on this seeded-key behavior more than bootstrap-admin behavior.

## First Access Checklist

The right first-access path depends on the runtime shape.

### Local development

- open `/admin`
- sign in with `admin@local` / `admin`
- inspect live admin-backed pages

### Production-shaped local run

- open `/admin`
- sign in with `admin@local` / `admin`
- rotate the password when prompted
- confirm provider secrets and Postgres connectivity

### Compose deploy

- confirm the stack is healthy
- confirm the seeded API key exists and works for `/v1/*`
- confirm whether admin bootstrap is enabled in the mounted config before assuming `/admin` is usable

## Startup Paths That Are Easy To Confuse

These behaviors are easy to blur together:

- `seed-config` creates config-backed runtime objects
- `bootstrap-admin` creates control-plane access
- `serve` can do both, but only when the related switches are on

That means one environment can have:

- valid API access but no admin login
- valid admin login but no seeded gateway key
- both
- neither, if the startup toggles are disabled and the database is empty

## Current Gaps

- Declarative users, teams, and budgets are not part of the config contract yet. That future direction is tracked in [issue #64](https://github.com/ahstn/oceans-llm/issues/64) and [issue #65](https://github.com/ahstn/oceans-llm/issues/65).
- Compose deploy does not ship a checked-in bootstrap-admin block by default.

## What This Page Does Not Own

- config field syntax and examples: [configuration-reference.md](../configuration/configuration-reference.md)
- deploy topology and runtime caveats: [deploy-and-operations.md](deploy-and-operations.md)
- step-by-step recovery and upgrade actions: [operator-runbooks.md](../operations/operator-runbooks.md)
- user lifecycle, onboarding, and OIDC policy: [identity-and-access.md](../access/identity-and-access.md)
