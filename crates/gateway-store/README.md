# gateway-store

`gateway-store` contains the gateway persistence layer and keeps the libsql/SQLite and PostgreSQL backends behind the same repository and migration interfaces.

- libsql/SQLite remains the default for plain local development and lightweight single-node runs.
- PostgreSQL is the default backend for production and pre-production shaped configs.
- Migrations, seed upserts, and bootstrap-oriented persistence behavior live here so both backends stay aligned at the application level.

For the runtime policy and the rollout rationale, see [`docs/adr/2026-03-09-runtime-database-policy.md`](../../docs/adr/2026-03-09-runtime-database-policy.md).

## Migration invariants

- The active runtime registry starts with one `V17` baseline migration per backend.
- Each migration version is applied inside an explicit per-migration transaction on both backends.
- Migration SQL, the `refinery_schema_history` insert, and the reader-compatibility row are in the same transaction boundary.
- Existing known `refinery_schema_history` rows are validated against the active registry before `status`, `check`, or `apply`. Newer versions are accepted only when their stored reader requirements permit this binary.
- The gateway also checks compatibility at startup when migrations are disabled. Pending migrations do not block startup.
- Databases carrying pre-baseline `V1` through `V16` history are not upgraded in place; recreate them before moving onto the `V17` baseline release.
- Any failure during SQL application or history recording must roll back both schema/data changes and history state for that version.
- Migration ordering and idempotency semantics are unchanged; failed versions remain pending until retried successfully.

## Declaring migration compatibility

The runner owns `gateway_migration_compatibility(version, min_reader_version)`;
it is not an application table. Every newly applied migration records its
reader requirement alongside its history row. Existing migrations through V81
retain strict compatibility: they require their own version as the reader's
registry head (the newest migration version the gateway knows).

For each migration after V81, add an entry to `READER_COMPATIBILITY` in
`src/migration_registry.rs`:

- `ReaderCompatibility::Additive` records minimum reader version `0`. Use it
  only when older gateways keep working, such as new tables, nullable columns,
  or indexes. Registry tests reject destructive SQL in additive migrations on
  either backend.
- `ReaderCompatibility::RequiresReader(v)` requires a gateway whose registry
  head is at least `v`, which must not exceed this migration's own version.

Use expand/contract for destructive changes: release N first stops reading or
writing the old column (and performs any required transition), with registry
head `v`. Release N+1 can drop the column with `RequiresReader(v)`, so release N
remains a safe rollback target. Never mark a destructive change additive.

An older gateway can check and run against a compatible newer database;
applying migrations is a no-op when all its known migrations are already
applied. It refuses to fill known migration gaps beneath newer history.
Missing metadata or an incompatible reader requirement blocks rollback without
changing the schema: deploy the newer release again or restore the backup
taken before the upgrade. Pre-compatibility binaries do not understand this
contract and still reject newer migration history.
