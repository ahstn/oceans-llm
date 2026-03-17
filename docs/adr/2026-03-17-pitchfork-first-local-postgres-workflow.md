# ADR: Pitchfork-First Local PostgreSQL Workflow

- Date: 2026-03-17
- Status: Accepted

## Context

PostgreSQL is now the default runtime backend for production-shaped and pre-production environments, and CI already runs PostgreSQL-backed checks. Local developer workflows were still primarily documented around Docker Compose for starting Postgres.

That created two problems:

- operational drift between docs and local process-manager tooling already available in this repo (`pitchfork` + `mise`),
- slower local iteration when contributors only need a local host Postgres daemon and not a full container stack.

We still need a supported fallback path for contributors who cannot run local Postgres binaries.

## Decision

### 1. Make pitchfork the default local Postgres workflow

The repository now defines a project `pitchfork.toml` with a `postgres` daemon and readiness check. New `mise` tasks wrap common lifecycle commands:

- `mise run postgres-start`
- `mise run postgres-stop`
- `mise run postgres-status`
- `mise run postgres-logs`
- `mise run postgres-reset`
- `mise run postgres-env`

### 2. Provision Postgres binaries with mise-managed tooling

`mise.toml` now pins a Postgres major version aligned with runtime policy so contributors can install required binaries with `mise install`.

### 3. Keep Docker Compose as an explicit fallback

Docker-based local Postgres remains documented for contributors who cannot run local binaries. Deployment assets remain Docker-oriented; this ADR only changes local dev/test defaults.

### 4. Standardize local URL exports

Pitchfork helper scripts now derive and expose deterministic local defaults for:

- `POSTGRES_URL`
- `TEST_POSTGRES_URL`

This keeps local smoke/testing commands aligned with CI-oriented Postgres checks.

## Consequences

Positive:

- local docs and workflow now match the intended Postgres-first runtime validation path,
- faster and lighter local startup for contributors using host binaries,
- deterministic URL contracts for Postgres tests and smoke checks.

Tradeoffs:

- contributors now rely on a still-evolving daemon tool (`pitchfork`) for the default local path,
- local binary provisioning can be slower on first install than container pull/start.

## Follow-up Work

- Monitor pitchfork reliability in daily usage and keep Docker fallback docs current.
- Revisit whether CI should adopt pitchfork for parity, or continue with service containers for stability.

## Attribution

This ADR was prepared through collaborative human + AI implementation/design work.
