# ADR: PostgreSQL Runtime Default Outside Local Development

- Date: 2026-03-09
- Status: Accepted

## Context

The gateway started with a local libsql/SQLite runtime, which was a good fit for single-node development and early product slices. That runtime was no longer a good default for production-shaped and pre-production environments because:

- deployment assets already provisioned a Postgres service,
- release validation was not exercising the database backend we intend to trust in production,
- operational workflows such as connection management, backup/restore, and concurrency behavior are materially stronger with PostgreSQL,
- the gateway still needed to preserve a lightweight local developer path that does not require standing up external infrastructure.

We also wanted to keep the rollout constrained:

- local development should stay simple,
- production and pre-production should converge on one runtime backend,
- gateway startup should remain responsible for migrations and idempotent seed bootstrapping,
- this slice should not expand into an automated SQLite/libsql-to-Postgres data migration project.

## Decision

### 1. Use PostgreSQL as the default runtime database for production and pre-production

Production-shaped configs and deploy assets now target PostgreSQL through `POSTGRES_URL`.

Why:
- matches the deployment topology we already ship,
- gives non-local environments the stronger concurrency and operational characteristics we expect,
- makes release validation exercise the intended runtime backend.

### 2. Keep libsql/SQLite as the default for plain local development

`gateway.yaml` remains the default local config and continues to use libsql/SQLite.

Why:
- keeps the shortest path for local setup,
- avoids making all contributors run Postgres for basic development,
- preserves the existing lightweight workflow for demos and single-node testing.

### 3. Keep migrations and seed bootstrapping in the gateway, not in the Postgres container

The gateway remains responsible for:

- running schema migrations at startup,
- ensuring bootstrap admin access,
- seeding providers, models, and API keys idempotently.

We explicitly do not preload application rows through `docker-entrypoint-initdb.d`.

Why:
- keeps bootstrapping logic in one place,
- avoids drift between container init SQL and application-level seed behavior,
- makes libsql and Postgres environments behave the same way from the application’s point of view.

### 4. Use fresh PostgreSQL cutover for this slice

This slice does not include an automated migration path from existing SQLite/libsql runtime data into PostgreSQL.

Why:
- backend support, migration safety, and release validation are the immediate goals,
- a reliable cross-backend data migration tool would materially expand scope,
- operators can adopt PostgreSQL for new or reset non-local environments first, then evaluate data migration separately.

### 5. Require PostgreSQL-backed validation in CI

Rust CI now provisions PostgreSQL and runs the workspace tests with a Postgres connection string available.

Why:
- prevents production-only backend drift,
- ensures new Postgres-backed tests become a merge gate instead of a manual check,
- keeps local compose available for manual troubleshooting without making Docker Compose a CI dependency.

## Consequences

Positive:
- non-local environments now converge on PostgreSQL as the supported runtime backend,
- local development remains low-friction,
- application bootstrap behavior stays consistent across backends,
- CI can catch Postgres-specific regressions before merge.

Tradeoffs:
- production-shaped local runs now require PostgreSQL to be available,
- automated migration of historical SQLite/libsql runtime data is deferred,
- the gateway must continue to own careful, backend-aware migration behavior.

## Follow-up Work

- Add and maintain Postgres-backed parity tests for the gateway and store layers.
- Revisit whether a one-time SQLite/libsql-to-Postgres migration utility is needed for existing deployments.
- Evaluate whether future operational needs justify more explicit migration/seed CLI commands beyond startup bootstrapping.

## Attribution

This ADR was prepared through collaborative human + AI implementation/design work.
