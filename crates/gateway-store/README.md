# gateway-store

`gateway-store` contains the gateway persistence layer and keeps the libsql/SQLite and PostgreSQL backends behind the same repository and migration interfaces.

- libsql/SQLite remains the default for plain local development and lightweight single-node runs.
- PostgreSQL is the default backend for production and pre-production shaped configs.
- Migrations, seed upserts, and bootstrap-oriented persistence behavior live here so both backends stay aligned at the application level.

For the runtime policy and the rollout rationale, see [`docs/adr/2026-03-09-runtime-database-policy.md`](../../docs/adr/2026-03-09-runtime-database-policy.md).

## Migration invariants

- Each migration version is applied inside an explicit per-migration transaction on both backends.
- Migration SQL and the `refinery_schema_history` insert are in the same transaction boundary.
- Any failure during SQL application or history recording must roll back both schema/data changes and history state for that version.
- Migration ordering and idempotency semantics are unchanged; failed versions remain pending until retried successfully.
