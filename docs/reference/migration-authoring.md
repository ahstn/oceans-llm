# Migration Authoring

`See also`: [Data Relationships](data-relationships.md), [Admin Runbooks](../operations/operator-runbooks.md), [Release Process](release-process.md), [ADR: Pre-v1 Store Migration Re-Baseline](../adr/2026-03-31-pre-v1-migration-rebaseline.md), [ADR: Migration Atomicity Hardening and Pitchfork-First Local Postgres Operations](../adr/2026-03-17-migration-atomicity-and-local-postgres-ops-hardening.md)

This page is the maintainer checklist for adding migrations after the active `V17` baseline.

## Source of Truth

- libsql and SQLite migrations:
  - [../crates/gateway-store/migrations/](../../crates/gateway-store/migrations)
- PostgreSQL migrations:
  - [../crates/gateway-store/migrations/postgres/](../../crates/gateway-store/migrations/postgres)
- store registry and migration runner:
  - [../crates/gateway-store/src/lib.rs](../../crates/gateway-store/src/lib.rs)
- local Postgres tasks:
  - [../mise.toml](../../mise.toml)

## Invariants

- Fresh databases start from one active `V17` baseline per backend.
- Databases with pre-baseline `V1` through `V16` history are recreated, not upgraded in place.
- New migrations use the next shared version number for both backends.
- libsql/SQLite and PostgreSQL migrations must stay logically equivalent.
- Migration SQL and migration-history writes must remain atomic.
- Store tests and smoke checks should cover both backends when schema behavior changes.

## Authoring Checklist

1. Pick the next migration version.
2. Add matching files under both migration directories.
3. Keep names descriptive, for example `V18__route_compatibility_profiles.sql`.
4. Update the migration registry if the runner requires an explicit include.
5. Add or update store tests for new constraints, indexes, and repository behavior.
6. Run the libsql/SQLite path locally.
7. Run the PostgreSQL path locally.
8. Update data-model docs when the entity graph changes.
9. Update admin/user docs when the migration changes visible behavior.
10. Add or update an ADR when the schema change reflects an architectural decision.

## Local Validation

Use mise tasks instead of ad hoc database commands.

```bash
mise run gateway-migrate
mise run postgres-start
mise run gateway-migrate-prod
mise run test-rust-postgres
mise run test-gateway-postgres-smoke
```

For full pre-handoff validation, use:

```bash
mise run lint
mise run test
```

## Documentation Triggers

Update [data-relationships.md](data-relationships.md) when a migration changes:

- ownership or identity relationships
- request-log or usage-ledger shape
- budget tables or enforcement inputs
- provider, model, route, capability, or compatibility tables
- generated admin API payloads that expose new persisted fields

Update [admin-runbooks](../operations/operator-runbooks.md) when the migration changes admin recovery, reset, upgrade, or first-access steps.

## What This Page Does Not Own

- schema relationship reference:
  - [data-relationships.md](data-relationships.md)
- recovery and reset procedures:
  - [operator-runbooks.md](../operations/operator-runbooks.md)
- release mechanics:
  - [release-process.md](release-process.md)
