# ADR: Migration Atomicity Hardening and Pitchfork-First Local Postgres Operations

- Date: 2026-03-17
- Status: Accepted

## Current state

- [../deploy-and-operations.md](../deploy-and-operations.md)
- [../runtime-bootstrap-and-access.md](../runtime-bootstrap-and-access.md)
- [../operator-runbooks.md](../operator-runbooks.md)

## Context

Two operational concerns were open at the same time:

1. Migration safety needed stronger guarantees and diagnostics around failures between migration SQL execution and schema-history writes.
2. Local Postgres operations for production-shaped validation were split across multiple paths, despite Postgres now being central to non-local runtime policy and CI coverage.

These concerns are related: safer migration execution is only useful if the team can consistently and quickly run Postgres-backed validation paths during day-to-day development.

## Decision

### 1. Harden per-migration atomicity behavior and observability

We keep one explicit transaction per migration version and enforce that:

- migration SQL (when present) and `refinery_schema_history` writes happen in the same transaction,
- any failure in apply/history stages rolls back both schema/data and history state,
- migration execution emits structured step-level logs (`begin`, `apply`, `history_insert`, `commit`, `rollback`) with backend + migration metadata.

How implemented:

- migration test hook now supports explicit history-insert failure injection,
- migration runner performs rollback on all stage failures and logs rollback outcomes,
- tests cover both backends for:
  - failure after SQL apply,
  - failure during history insert,
  - status recovery after failure and successful retry.

### 2. Make local Postgres operations pitchfork-first and singular

For local development and validation, we use `pitchfork` as the single supported Postgres lifecycle path in this iteration.

How implemented:

- root `pitchfork.toml` defines a project `postgres` daemon with readiness checks,
- new helper script bootstraps `initdb` idempotently, ensures database creation, and exports deterministic URL envs,
- `mise` adds Postgres tool pinning and local lifecycle tasks (`postgres-start`, `postgres-stop`, `postgres-status`, `postgres-logs`, `postgres-reset`, `postgres-env`),
- README now leads with pitchfork workflow and removes alternate local Postgres startup tracks.

### 3. Add explicit release-readiness gating for Postgres parity

We codify merge/release expectations in review workflow by adding a PR checklist requiring Postgres-backed checks when runtime/store/migration/release behavior changes.

## Why this approach

- Preserves existing runtime behavior while reducing migration failure ambiguity and rollback risk.
- Improves developer throughput by removing the need for container-only local Postgres startup in the common case.
- Reduces contributor ambiguity by removing duplicated local startup paths and standardizing around one deterministic workflow.
- Makes Postgres parity expectations explicit for reviewers, reducing policy drift over time.

## Consequences

Positive:

- clearer operational diagnostics for migration incidents,
- stronger regression coverage for migration failure modes,
- faster and more consistent local Postgres validation path,
- tighter review discipline around Postgres parity.

Tradeoffs:

- pitchfork is still a newer operational dependency for local workflows.
- contributors must install local Postgres binaries via `mise` before running Postgres-backed checks.

## Follow-up Work

- Monitor pitchfork reliability in contributor workflows and adjust tooling/scripts if startup friction appears.
- Reassess whether CI should remain service-container-based or adopt pitchfork parity later.
- Continue to add migration-stage tests when new migration behavior is introduced.

## Attribution

This ADR was prepared through collaborative human + AI implementation/design work.
