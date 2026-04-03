# ADR: Pre-v1 Store Migration Re-Baseline

- Date: 2026-03-31
- Status: Accepted

## Implemented By

- Runtime migration registry:
  - [../../crates/gateway-store/src/migration_registry.rs](../../crates/gateway-store/src/migration_registry.rs)
  - [../../crates/gateway-store/src/migrate.rs](../../crates/gateway-store/src/migrate.rs)
- Active baseline SQL:
  - [../../crates/gateway-store/migrations/V17__baseline.sql](../../crates/gateway-store/migrations/V17__baseline.sql)
  - [../../crates/gateway-store/migrations/postgres/V17__baseline.sql](../../crates/gateway-store/migrations/postgres/V17__baseline.sql)
- Canonical operator docs:
  - [../setup/deploy-and-operations.md](../setup/deploy-and-operations.md)
  - [../operations/operator-runbooks.md](../operations/operator-runbooks.md)

## Context

The store accumulated a pre-v1 migration chain (`V1` through `V16`) while the libsql and PostgreSQL backends were converging on the same logical schema. That history had two problems:

- the runtime path carried compatibility-era steps and placeholder migrations that no longer represented the current schema contract,
- operators could end up with old history rows that looked accepted even though the active codebase only cared about the latest end-state schema.

This created unnecessary runtime complexity, misleading migration status output, and extra test burden around upgrade paths we do not want to support before `v1`.

## Decision

### 1. Re-baseline the active store schema at `V17`

We replace the runtime migration registry with one active `V17__baseline` migration per backend, built from the current `main` schema.

Why:

- keeps the active migration path aligned with the schema we actually support,
- removes compatibility-only steps from the hot path,
- makes libsql and PostgreSQL parity easier to reason about and test.

### 2. Drop pre-baseline runtime compatibility shims

We remove compatibility/no-op migration steps from the registry. Every active registry entry now maps to real backend SQL.

Why:

- fallback shims preserve old patterns and make the migration system harder to trust,
- status output should describe real schema work, not compatibility bookkeeping,
- keeping old no-op versions alive would encourage future migration debt.

### 3. Treat old migration history as reset-only

Databases carrying pre-baseline `V1` through `V16` history are no longer upgraded in place. Before `status`, `check`, or `apply`, the migration runner validates `refinery_schema_history` against the active manifest identity. Unknown versions or manifest mismatches fail with a clear reset-required error.

Why:

- before `v1`, recreate-on-upgrade is safer than pretending the old chain is still supported,
- explicit failure is better than silent partial compatibility,
- manifest validation makes migration identity enforceable instead of advisory.

### 4. Keep `system-legacy` only as live seeded ownership semantics

The reserved `system-legacy` team remains only for seeded system-owned API keys. It is not part of the active migration story anymore.

Why:

- seeded/system-owned keys still need a stable team owner in the runtime model,
- migration-time backfill behavior should not survive once the old upgrade path is intentionally removed.

## Consequences

Positive:

- the active migration system is simpler and stricter,
- migration status/check/apply behavior is deterministic across both backends,
- docs and tests can focus on the supported fresh-database path,
- Git history becomes the archive for the pre-baseline chain instead of the live runtime tree.

Tradeoffs:

- old pre-v1 databases must be recreated during upgrade,
- the project intentionally gives up in-place upgrade support for historical development databases.

## Follow-up Work

- Add future schema changes as post-baseline migrations after `V17`.
- Keep docs and operator guidance explicit whenever migration policy changes again.
- Continue enforcing backend parity through fresh-database migration tests on both libsql and PostgreSQL.
