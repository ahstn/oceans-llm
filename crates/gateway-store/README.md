# gateway-store

`gateway-store` contains the gateway persistence layer and keeps the libsql/SQLite and PostgreSQL backends behind the same repository and migration interfaces.

- libsql/SQLite remains the default for plain local development and lightweight single-node runs.
- PostgreSQL is the default backend for production and pre-production shaped configs.
- Migrations, seed upserts, and bootstrap-oriented persistence behavior live here so both backends stay aligned at the application level.

For the runtime policy and the rollout rationale, see [`docs/adr/2026-03-09-runtime-database-policy.md`](../../docs/adr/2026-03-09-runtime-database-policy.md).
