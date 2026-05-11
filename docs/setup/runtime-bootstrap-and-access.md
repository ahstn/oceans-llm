# Runtime Bootstrap and Access

`See also`: [Configuration Reference](../configuration/configuration-reference.md), [Identity and Access](../access/identity-and-access.md), [Service Accounts](../access/service-accounts.md), [Oceans LLM Gateway](../../README.md), [Deploy](../../deploy/README.md), [Deploy and Operations](deploy-and-operations.md), [Admin Runbooks](../operations/operator-runbooks.md)

This page explains what the gateway does when it starts and what access exists right after boot.

## Source of Truth

- CLI entry points: [../crates/gateway/src/main.rs](../../crates/gateway/src/main.rs)
- Runtime commands: [../mise.toml](../../mise.toml)
- Checked-in configs: [../gateway.yaml](../../gateway.yaml), [../gateway.prod.yaml](../../gateway.prod.yaml), [../deploy/config/gateway.yaml](../../deploy/config/gateway.yaml)

## Startup Actions

The normal startup path is `gateway serve`.

At startup, the gateway can do three separate things:

- run migrations
- seed config-backed objects such as providers, models, teams, users, and budgets
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

| Runtime shape | Config | Required env for first access | Database | Admin login source | Data-plane access source |
| --- | --- | --- | --- | --- | --- |
| Local development | `gateway.yaml` | none for checked-in defaults | libsql or SQLite | `admin@local` / `admin` from checked-in config | none by default; local demo reset seeds sample keys |
| Production-shaped local | `gateway.prod.yaml` | `POSTGRES_URL` if not supplied by the mise task | PostgreSQL | `admin@local` / `admin`, with forced password rotation | service account created after admin access |
| GHCR compose deploy | `deploy/config/gateway.yaml` | `GATEWAY_BOOTSTRAP_ADMIN_PASSWORD`, provider secrets, Postgres env | PostgreSQL | `admin@local` with env-backed bootstrap password | service account created after admin access |
| Kubernetes Helm deploy | rendered `gateway.config` values | `POSTGRES_URL` or CloudNativePG connection secret, `GATEWAY_IDENTITY_TOKEN_SECRET`, provider secrets | PostgreSQL | opt-in bootstrap-admin Job | service account created after admin access |

Bootstrap admin access and service-account data-plane access are separate paths. One can exist without the other.

### Local development

- config: [../gateway.yaml](../../gateway.yaml)
- database: libsql or SQLite
- usual entry point: `mise run dev-stack`
- bootstrap admin: enabled
- forced password change: off
- seeded teams, users, and budgets: driven by config

What exists after boot:

- a gateway on `http://localhost:8080`
- an admin UI at `http://localhost:8080/admin`
- a bootstrap admin at `admin@local` with password `admin`
- config-seeded local users are invited team members, not checked-in control-plane admins
- when `./gateway.db` is absent and you start with `mise run dev-stack`, the local demo dataset is seeded automatically:
  - 2 teams
  - 5 users across those teams
  - owner-aware demo credentials for local data-plane testing
  - $1000/month budgets for each team and a $50/day user budget
  - request-log and spend history sample rows for the admin observability pages

### Production-shaped local run

- config: [../gateway.prod.yaml](../../gateway.prod.yaml)
- database: PostgreSQL
- usual entry point: `mise run prod-stack`
- bootstrap admin: enabled
- forced password change: on
- data-plane service accounts: created after admin access

What exists after boot:

- the same admin email and password as the local-dev config
- a forced password rotation on first sign-in
- a runtime shape closer to deploy and release behavior

### GHCR compose deploy

- compose entry point: [../deploy/compose.yaml](../../deploy/compose.yaml)
- mounted config: [../deploy/config/gateway.yaml](../../deploy/config/gateway.yaml)
- database: PostgreSQL
- bootstrap admin: enabled in the checked-in deploy config
- seeded teams, users, and budgets: defined in the mounted config

What exists after boot:

- a bootstrap admin at `admin@local`
- config-seeded teams, invited users, and any listed active budgets
- admin access once `GATEWAY_BOOTSTRAP_ADMIN_PASSWORD` is set in the deploy environment
- data-plane access after an allowed admin creates a service account and credential

### Kubernetes Helm deploy

- chart source: [../../deploy/helm/oceans-llm](../../deploy/helm/oceans-llm/README.md)
- mounted config: rendered from `gateway.config` values
- database: external PostgreSQL by default, optional CloudNativePG `Cluster`
- migrations: Helm hook Job enabled by default
- bootstrap admin: disabled until `bootstrapAdminJob.enabled=true`
- seeded config: disabled until `seedConfigJob.enabled=true`

What exists after boot depends on the enabled Jobs:

- migrations run before normal external-Postgres installs and after CloudNativePG resource creation for CloudNativePG installs
- gateway pods do not run migrations, bootstrap admin creation, or config seeding on startup
- admin access only exists if bootstrap-admin is enabled or an admin already exists in the database
- data-plane access exists after an allowed admin creates a service account and credential

## Bootstrap Admin

Bootstrap admin creation is for control-plane access.

- It is separate from service-account data-plane credentials.
- It is checked at startup when bootstrap behavior is enabled.
- It can also be created with `gateway bootstrap-admin`.

Checked-in defaults:

- local config keeps the bootstrap admin on and does not force password rotation
- production-shaped local config keeps the bootstrap admin on and forces password rotation

For the lifecycle and ownership rules after that first sign-in, see [identity-and-access.md](../access/identity-and-access.md).

## Service-Account Data-Plane Access

Service accounts are for non-human data-plane access.

- They belong to a team.
- They are managed by platform admins or by owners and admins of the owning team.
- They authenticate through issued credentials.
- They do not replace admin login.
- Direct team-owned runtime API keys and `system-legacy` seeded keys are not supported.

Seeded users and teams are also config-backed, but password and OIDC onboarding links are still generated through the admin UI after boot.

## Access Matrix

| Object | Can sign in to `/admin`? | Can call `/v1/*`? | Notes |
| --- | --- | --- | --- |
| Bootstrap admin | Yes | No, unless a separate user credential is created | First control-plane access path. |
| Config-seeded invited user | No, until onboarding completes | No, unless a credential is created for that user | Starts as `invited`. |
| Config-seeded platform admin user | No, until onboarding completes | No, unless a credential is created for that user | Role is seeded, but auth proof still requires onboarding. |
| Gateway service account | No | Yes, through an issued credential | Team-owned non-human principal. |
| Service-account credential | No | Yes | Data-plane access only. |
| Direct team-owned API key | No | No | Removed compatibility path. |

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
- sign in with `admin@local` and the configured bootstrap password
- inspect seeded teams, invited users, and budgets in `/admin`
- create a team service account and credential for automation that needs `/v1/*`

### Kubernetes Helm deploy

- render the intended values with `mise run helm-template` or `helm template`
- confirm required secrets exist before install or are produced by `ExternalSecret`
- confirm the migration Job succeeds
- confirm gateway and admin UI pods become ready
- call `/healthz` and `/readyz` through the gateway service or ingress
- sign in to `/admin` only after bootstrap-admin is intentionally enabled or a pre-existing admin exists

## Startup Paths That Are Easy To Confuse

These behaviors are easy to blur together:

- `seed-config` creates config-backed runtime objects
- `bootstrap-admin` creates control-plane access
- `serve` can do both, but only when the related switches are on
- service-account credentials create non-human data-plane access

That means one environment can have:

- valid API access but no admin login
- valid admin login but no service-account credential
- both
- neither, if the startup toggles are disabled and the database is empty

## Current Gaps

- Hardened declarative SSO-backed identity matching is still tracked in [issue #65](https://github.com/ahstn/oceans-llm/issues/65).

## What This Page Does Not Own

- config field syntax and examples: [configuration-reference.md](../configuration/configuration-reference.md)
- deploy topology and runtime caveats: [deploy-and-operations.md](deploy-and-operations.md)
- Kubernetes chart contract: [kubernetes-and-helm.md](kubernetes-and-helm.md)
- step-by-step recovery and upgrade actions: [operator-runbooks.md](../operations/operator-runbooks.md)
- user lifecycle, onboarding, and OIDC policy: [identity-and-access.md](../access/identity-and-access.md)
